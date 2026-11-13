use std::collections::HashMap;
use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use tokio::sync::{Mutex, RwLock, mpsc, oneshot};
use uuid::Uuid;

use crate::agent::{self, Model, OpenRouterModel};
use crate::session::Session;

type AcpError = Box<dyn Error + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
struct SessionId(Uuid);

impl SessionId {
    fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

struct SessionEntry {
    session: Mutex<Session>,
    active_turn_stop: Mutex<Option<oneshot::Sender<()>>>,
}

impl SessionEntry {
    fn new(session: Session) -> Self {
        Self {
            session: Mutex::new(session),
            active_turn_stop: Mutex::new(None),
        }
    }
}

type SessionHandle = Arc<SessionEntry>;
type SessionRegistry = Arc<RwLock<HashMap<SessionId, SessionHandle>>>;

async fn get_session(registry: &SessionRegistry, session_id: &SessionId) -> Option<SessionHandle> {
    let guard = registry.read().await;
    guard.get(session_id).cloned()
}

const INVALID_PARAMS_CODE: i32 = -32602;
const CONNECTION_NOT_INITIALIZED_CODE: i32 = -32600;
const METHOD_NOT_FOUND_CODE: i32 = -32601;
const RESOURCE_NOT_FOUND_CODE: i32 = -32002;
const INTERNAL_ERROR_CODE: i32 = -32603;

struct ConnectionState {
    initialized: bool,
    sessions: SessionRegistry,
}

impl ConnectionState {
    fn new() -> Self {
        Self {
            initialized: false,
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[derive(Debug, Deserialize)]
struct Notification<T> {
    method: String,
    params: Option<T>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CancelParams {
    session_id: SessionId,
}

#[derive(Debug, Deserialize)]
struct Request<T> {
    id: Value,

    #[allow(dead_code)]
    method: String,

    #[allow(dead_code)]
    params: Option<T>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct InitializeParams {
    protocol_version: u16,

    client_info: Option<Implementation>,

    #[serde(default)]
    client_capabilities: ClientCapabilities,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NewSessionParams {
    cwd: PathBuf,

    #[allow(
        dead_code,
        reason = "ACP requires mcpServers, but MCP support is intentionally deferred"
    )]
    mcp_servers: Vec<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PromptParams {
    session_id: SessionId,
    prompt: Vec<PromptContentBlock>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum PromptContentBlock {
    Text {
        text: String,
    },
    #[serde(other)]
    Unsupported,
}

fn join_text_blocks(blocks: Vec<PromptContentBlock>) -> Option<String> {
    let mut text_blocks = Vec::new();

    for block in blocks {
        match block {
            PromptContentBlock::Text { text } => text_blocks.push(text),
            PromptContentBlock::Unsupported => return None,
        }
    }

    Some(text_blocks.join("\n"))
}

fn parse_prompt_params(params: Option<Value>) -> Result<(SessionId, String), ()> {
    let PromptParams { session_id, prompt } =
        serde_json::from_value(params.unwrap_or(Value::Null)).map_err(|_| ())?;

    if prompt.is_empty() {
        return Err(());
    }

    let prompt = join_text_blocks(prompt).ok_or(())?;

    Ok((session_id, prompt))
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
#[allow(dead_code)]
struct FileSystemCapabilities {
    read_text_file: bool,
    write_text_file: bool,
}

#[derive(Debug, Default, Deserialize)]
#[allow(dead_code)]
#[serde(default)]
struct AuthCapabilities {
    terminal: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
#[allow(dead_code)]
struct ClientCapabilities {
    fs: FileSystemCapabilities,
    auth: AuthCapabilities,
    terminal: bool,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct Implementation {
    name: String,
    title: Option<String>,
    version: String,
}

#[derive(Debug, Serialize)]
struct SuccessResponse<T> {
    jsonrpc: String,
    id: Value,
    result: T,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    jsonrpc: String,
    id: Value,
    error: ResponseError,
}

impl ErrorResponse {
    fn new(id: Value, code: i32, message: &str) -> Self {
        Self {
            jsonrpc: String::from("2.0"),
            id,
            error: ResponseError {
                code,
                message: message.to_string(),
            },
        }
    }
    fn invalid_params(id: Value) -> Self {
        Self::new(id, INVALID_PARAMS_CODE, "Invalid params")
    }

    fn connection_not_initialized(id: Value) -> Self {
        Self::new(
            id,
            CONNECTION_NOT_INITIALIZED_CODE,
            "Connection not initialized",
        )
    }

    fn method_not_found(id: Value) -> Self {
        Self::new(id, METHOD_NOT_FOUND_CODE, "Method not found")
    }

    fn resource_not_found(id: Value) -> Self {
        Self::new(id, RESOURCE_NOT_FOUND_CODE, "Resource not found")
    }
}

#[derive(Debug, Serialize)]
struct ResponseError {
    code: i32,
    message: String,
}

#[derive(Debug, PartialEq, Eq)]
enum MessageKind {
    Request,
    Notification,
    Response,
}

const MAX_FRAME_BYTES: usize = 64 * 1024;

fn classify_message(value: &Value) -> Result<MessageKind, AcpError> {
    let object = value.as_object().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "JSON-RPC message must be an object.",
        )
    })?;

    let has_method = object.contains_key("method");
    let has_id = object.contains_key("id");
    let has_result = object.contains_key("result");
    let has_error = object.contains_key("error");

    match (has_method, has_id, has_result, has_error) {
        (true, true, false, false) => Ok(MessageKind::Request),
        (true, false, false, false) => Ok(MessageKind::Notification),
        (false, true, true, false) | (false, true, false, true) => Ok(MessageKind::Response),
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid JSON-RPC message shape",
        )
        .into()),
    }
}

fn validate_request(request: &Request<Value>) -> Result<(), AcpError> {
    let is_supported_id = match &request.id {
        Value::String(_) => true,
        Value::Number(number) => number.is_u64() || number.is_i64(),
        _ => false,
    };

    if !is_supported_id {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "request ID must be a string or integer",
        )
        .into());
    }

    Ok(())
}

fn validate_version(value: &Value) -> Result<(), AcpError> {
    match value.get("jsonrpc").and_then(|version| version.as_str()) {
        Some("2.0") => Ok(()),
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "expected JSON-RPC version 2.0",
        )
        .into()),
    }
}

async fn write_frame<W: AsyncWrite + Unpin>(writer: &mut W, value: &Value) -> Result<(), AcpError> {
    let mut frame = serde_json::to_vec(value)?;
    frame.push(b'\n');

    writer.write_all(&frame).await?;
    writer.flush().await?;

    Ok(())
}

async fn write_frames<W: AsyncWrite + Unpin + Send>(
    mut writer: W,
    mut receiver: mpsc::Receiver<Value>,
) -> Result<(), AcpError> {
    while let Some(value) = receiver.recv().await {
        write_frame(&mut writer, &value).await?;
    }

    Ok(())
}

async fn read_frames<R, M>(
    mut reader: R,
    sender: mpsc::Sender<Value>,
    mut state: ConnectionState,
    model: M,
) -> Result<(), AcpError>
where
    R: AsyncBufRead + Unpin,
    M: Model + Send + Sync + 'static,
{
    let mut input = String::new();
    let model = Arc::new(model);

    loop {
        input.clear();
        let n = {
            let mut limited_reader = (&mut reader).take((MAX_FRAME_BYTES + 1) as u64);

            limited_reader.read_line(&mut input).await?
        };

        if n == 0 {
            break;
        }

        if n > MAX_FRAME_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "ACP frame exceeds maximum size",
            )
            .into());
        }

        if !input.ends_with('\n') {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "the last message should have a newline",
            )
            .into());
        }

        let message: Value = serde_json::from_str(&input)?;
        validate_version(&message)?;
        match classify_message(&message)? {
            MessageKind::Request => {
                let request: Request<Value> = serde_json::from_str(&input)?;
                validate_request(&request)?;

                if let Some(response) =
                    dispatch_request(request, &mut state, &model, &sender).await?
                {
                    sender.send(response).await?;
                }
            }
            MessageKind::Notification => {
                let notification: Notification<Value> = serde_json::from_str(&input)?;
                if notification.method == "session/cancel" {
                    let params = notification.params.unwrap_or(Value::Null);

                    // todo: What does ACP say about notification errors?
                    if let Ok(params) = serde_json::from_value::<CancelParams>(params)
                        && let Some(session) =
                            get_session(&state.sessions, &params.session_id).await
                    {
                        let turn_stop = {
                            let mut guard = session.active_turn_stop.lock().await;
                            guard.take()
                        };

                        if let Some(turn_stop) = turn_stop {
                            let _ = turn_stop.send(());
                        }
                    }
                }
            }

            MessageKind::Response => {}
        }
    }

    Ok(())
}

pub async fn run_acp<R, W>(reader: R, writer: W) -> Result<(), AcpError>
where
    R: AsyncBufRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let model = OpenRouterModel::new();

    run_acp_with_model(reader, writer, model).await
}

pub async fn run_acp_with_model<R, W, M>(reader: R, writer: W, model: M) -> Result<(), AcpError>
where
    R: AsyncBufRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
    M: Model + Send + Sync + 'static,
{
    let (sender, receiver) = mpsc::channel(1);
    let state = ConnectionState::new();
    let mut reader_task = tokio::spawn(read_frames(reader, sender, state, model));
    let mut writer_task = tokio::spawn(write_frames(writer, receiver));

    tokio::select! {
        reader_result = &mut reader_task => {
            let writer_result = writer_task.await?;
            writer_result?;

            let reader_result = reader_result?;
            reader_result?;
        }
        writer_result = &mut writer_task => {
            match writer_result {
                Ok(Ok(())) => {
                    let reader_result = reader_task.await?;
                    reader_result?;
                },
                Ok(Err(writer_error)) => {
                    reader_task.abort();

                    match reader_task.await {
                        Err(join_error) if join_error.is_cancelled() => {}
                        Ok(_) => {},
                        Err(join_error) => return Err(join_error.into()),
                    }

                    return Err(writer_error);
                }
                Err(writer_join_error) => {
                    reader_task.abort();

                    match reader_task.await {
                        Err(join_error) if join_error.is_cancelled() => {}
                        Ok(_) => {},
                        Err(join_error) => return Err(join_error.into()),
                    }

                    return Err(writer_join_error.into());
                }
            }
        }
    }

    Ok(())
}

async fn handle_initialize(
    request: Request<Value>,
    state: &mut ConnectionState,
) -> Result<SuccessResponse<Value>, ErrorResponse> {
    let params = request.params.unwrap_or(Value::Null);

    if serde_json::from_value::<InitializeParams>(params).is_err() {
        let response = ErrorResponse::invalid_params(request.id);
        return Err(response);
    }

    state.initialized = true;
    let result = serde_json::json!({
        "protocolVersion": 1,
        "agentCapabilities": {},
        "authMethods": []
    });

    let response = SuccessResponse {
        jsonrpc: String::from("2.0"),
        id: request.id,
        result,
    };

    Ok(response)
}

async fn handle_new_session(
    request: Request<Value>,
    state: &mut ConnectionState,
) -> Result<SuccessResponse<Value>, ErrorResponse> {
    let params = request.params.unwrap_or(Value::Null);
    let NewSessionParams {
        cwd,
        mcp_servers: _,
    } = match serde_json::from_value::<NewSessionParams>(params) {
        Ok(params) => params,
        Err(_) => {
            let response = ErrorResponse::invalid_params(request.id);
            return Err(response);
        }
    };

    if !cwd.is_absolute() {
        let response = ErrorResponse::invalid_params(request.id);
        return Err(response);
    }

    let session_id = SessionId::new();
    let session_entry = Arc::new(SessionEntry::new(Session::empty(cwd)));

    {
        let mut sessions = state.sessions.write().await;
        sessions.insert(session_id.clone(), session_entry);
    }

    let result = serde_json::json!({
        "sessionId": session_id,
    });
    let response = SuccessResponse {
        jsonrpc: String::from("2.0"),
        id: request.id,
        result,
    };

    Ok(response)
}

async fn dispatch_request<M: Model + Send + Sync + 'static>(
    request: Request<Value>,
    state: &mut ConnectionState,
    model: &Arc<M>,
    sender: &mpsc::Sender<Value>,
) -> Result<Option<Value>, AcpError> {
    if request.method == "initialize" {
        let result = match handle_initialize(request, state).await {
            Ok(response) => serde_json::to_value(response)?,
            Err(err) => serde_json::to_value(err)?,
        };

        return Ok(Some(result));
    }

    if !state.initialized {
        let response = ErrorResponse::connection_not_initialized(request.id);

        let response = serde_json::to_value(response)?;
        return Ok(Some(response));
    }

    if request.method == "session/new" {
        let result = match handle_new_session(request, state).await {
            Ok(response) => serde_json::to_value(response)?,
            Err(err) => serde_json::to_value(err)?,
        };

        Ok(Some(result))
    } else if request.method == "session/prompt" {
        let value = request.params.unwrap_or(Value::Null);
        let (session_id, prompt) = match parse_prompt_params(Some(value)) {
            Ok(params) => params,
            Err(_) => {
                let response = ErrorResponse::invalid_params(request.id);

                return Ok(Some(serde_json::to_value(response)?));
            }
        };

        return match get_session(&state.sessions, &session_id).await {
            Some(session_entry) => {
                let model = model.clone();
                let sender = sender.clone();

                tokio::spawn(async move {
                    let mut session_guard = session_entry.session.lock().await;
                    let history_length = session_guard.conversation.messages.len();
                    let (stop_sender, stop_receiver) = oneshot::channel::<()>();

                    {
                        let mut turn_stop = session_entry.active_turn_stop.lock().await;
                        turn_stop.replace(stop_sender);
                    }

                    let turn_result = tokio::select! {
                        result = agent::turn(model.as_ref(), &mut session_guard, &prompt) => {
                                Some(result)
                            }

                            _ = stop_receiver => {
                                None
                            }
                    };

                    {
                        // Clear the active-turn cancellation sender.
                        let mut turn_stop = session_entry.active_turn_stop.lock().await;
                        let _ = turn_stop.take();
                    }

                    match turn_result {
                        Some(result) => {
                            let answer = match result {
                                Ok(answer) => answer,
                                Err(error) => {
                                    let message = format!("Request {} failed: {error}", request.id);

                                    let response = ErrorResponse::new(
                                        request.id,
                                        INTERNAL_ERROR_CODE,
                                        &message,
                                    );
                                    sender.send(serde_json::to_value(response)?).await?;

                                    return Ok(());
                                }
                            };

                            let session_update = serde_json::json!({
                                "jsonrpc": "2.0",
                                "method": "session/update",
                                "params": {
                                    "sessionId": &session_id.0.to_string(),
                                    "update": {
                                        "content": {
                                            "type": "text",
                                            "text": answer
                                        },
                                        "sessionUpdate": "agent_message_chunk",
                                    }
                                }
                            });

                            sender.send(session_update).await?;

                            let response = SuccessResponse {
                                jsonrpc: String::from("2.0"),
                                id: request.id,
                                result: serde_json::json!({
                                    "stopReason": "end_turn"
                                }),
                            };

                            sender.send(serde_json::to_value(response)?).await?;

                            Ok::<(), AcpError>(())
                        }
                        None => {
                            session_guard.conversation.messages.truncate(history_length);

                            let response = SuccessResponse {
                                jsonrpc: String::from("2.0"),
                                id: request.id,
                                result: serde_json::json!({
                                    "stopReason": "cancelled"
                                }),
                            };

                            sender.send(serde_json::to_value(response)?).await?;

                            Ok::<(), AcpError>(())
                        }
                    }
                });

                Ok(None)
            }
            None => {
                let response = ErrorResponse::resource_not_found(request.id);

                Ok(Some(serde_json::to_value(response)?))
            }
        };
    } else {
        let response = ErrorResponse::method_not_found(request.id);
        let response = serde_json::to_value(response)?;
        Ok(Some(response))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{HashMap, VecDeque},
        io::Cursor,
        path::PathBuf,
        sync::Arc,
        time::Duration,
    };

    use serde_json::Value;
    use tokio::{
        io::{
            AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, BufWriter, DuplexStream,
            ReadHalf, WriteHalf, duplex, split,
        },
        sync::{Notify, RwLock, mpsc},
    };

    use uuid::Uuid;

    struct InertModel;

    impl Model for InertModel {
        async fn complete(
            &self,
            _request: Value,
        ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
            panic!("Should not be called")
        }
    }

    use crate::{
        acp::{
            CONNECTION_NOT_INITIALIZED_CODE, ConnectionState, INVALID_PARAMS_CODE,
            InitializeParams, MAX_FRAME_BYTES, METHOD_NOT_FOUND_CODE, MessageKind,
            PromptContentBlock, RESOURCE_NOT_FOUND_CODE, Request, SessionEntry, SessionId,
            classify_message, dispatch_request, get_session, join_text_blocks, parse_prompt_params,
            run_acp, run_acp_with_model, validate_version, write_frame,
        },
        agent::Model,
        scripted_model::{
            ScriptedModel,
            TurnResponse::{Done, TimeGated},
        },
        session::Session,
        test_messages::{
            assistant_reply, make_agent_message_chunk, make_cancel_notification, make_initialize,
            make_prompt_request,
        },
    };

    async fn read_frame(reader: &mut BufReader<ReadHalf<DuplexStream>>) -> Value {
        tokio::time::timeout(Duration::from_secs(1), async {
            let mut received = String::new();
            match reader.read_line(&mut received).await {
                Ok(0) => panic!("Received EOF instead of a complete frame"),
                Ok(_) => {
                    assert!(received.ends_with("\n"));
                    serde_json::from_str(&received).expect("frame should be valid JSON")
                }
                Err(error) => panic!("Unable to read a frame: {}", error),
            }
        })
        .await
        .expect("to read a JSON RPC frame")
    }

    async fn send_frame(writer: &mut WriteHalf<DuplexStream>, data: &Value) {
        let mut frame = serde_json::to_string(data).unwrap();
        frame.push('\n');

        writer
            .write_all(frame.as_bytes())
            .await
            .expect("frame should serialize");
    }

    async fn initialize(
        client_writer: &mut WriteHalf<DuplexStream>,
        client_reader: &mut BufReader<ReadHalf<DuplexStream>>,
    ) {
        let initialize_request = serde_json::json!({
          "jsonrpc": "2.0",
          "id": 1,
          "method": "initialize",
          "params": {
              "protocolVersion": 1,
          }
        });

        send_frame(client_writer, &initialize_request).await;

        let response = read_frame(client_reader).await;

        validate_version(&response).expect("response should use JSON-RPC 2.0");
        assert_eq!(response["id"], serde_json::json!(1));
        assert!(response["error"].is_null());
        assert_eq!(response["result"]["protocolVersion"], serde_json::json!(1));
    }

    async fn create_new_session(
        client_writer: &mut WriteHalf<DuplexStream>,
        client_reader: &mut BufReader<ReadHalf<DuplexStream>>,
        request_id: u64,
    ) -> String {
        let new_session_request = serde_json::json!({
          "jsonrpc": "2.0",
          "id": request_id,
          "method": "session/new",
          "params": {
            "cwd": "/tmp",
            "mcpServers": []
          }
        });

        send_frame(client_writer, &new_session_request).await;
        let result = read_frame(client_reader).await;
        validate_version(&result).expect("response should use JSON-RPC 2.0");

        assert_eq!(result["id"], serde_json::json!(request_id));
        assert!(
            result.get("error").is_none(),
            "a success response must not contain an error"
        );

        let session_id = result["result"]["sessionId"]
            .as_str()
            .expect("session/new should return a string session ID");
        Uuid::parse_str(session_id).expect("a valid UUID");

        session_id.to_string()
    }

    #[test]
    fn omitted_client_capabilities_uses_default() {
        let value = serde_json::json!({
          "jsonrpc": "2.0",
          "id": 1,
          "method": "initialize",
          "params": {
              "protocolVersion": 1,
          }
        });

        let request: Request<InitializeParams> =
            serde_json::from_value(value).expect("to serialize");

        let params = request.params.expect("to have client capabilities");
        assert!(!params.client_capabilities.fs.read_text_file);
        assert!(!params.client_capabilities.fs.write_text_file);
        assert!(!params.client_capabilities.auth.terminal);
        assert!(!params.client_capabilities.terminal);
    }

    #[tokio::test]
    async fn initialize_must_have_a_protocol_version() {
        let request = serde_json::json!({
          "jsonrpc": "2.0",
          "id": 1,
          "method": "initialize",
          "params": {
              "clientCapabilities": {},
          }
        });

        let mut input = serde_json::to_string(&request).expect("request should serialize");
        input.push('\n');
        let reader = Cursor::new(input.into_bytes());
        let (mut client_end, agent_end) = duplex(1024);

        let agent_output = BufWriter::new(agent_end);

        run_acp(reader, agent_output)
            .await
            .expect("request should succeed");

        let mut received = Vec::new();
        client_end
            .read_to_end(&mut received)
            .await
            .expect("client should read agent output");

        let result = serde_json::from_str(&String::from_utf8(received).expect("to be a string"))
            .expect("to be a valid message");

        validate_version(&result).expect("response should use JSON-RPC 2.0");

        assert_eq!(result["id"], serde_json::json!(1));
        assert!(
            result.get("result").is_none(),
            "an error response must not contain a result"
        );

        let error = &result["error"];
        assert_eq!(error["code"], -32602);
        assert_eq!(error["message"], "Invalid params")
    }

    #[tokio::test]
    async fn session_creation_before_initialization_is_rejected() {
        let request = Request {
            id: serde_json::json!(1),
            method: String::from("session/new"),
            params: Some(serde_json::json!({
                "cwd": "/tmp",
                "mcpServers": [],
            })),
        };

        let mut state = ConnectionState::new();
        let model = Arc::new(InertModel);
        let (sender, _) = mpsc::channel(2);

        let response = dispatch_request(request, &mut state, &model, &sender)
            .await
            .expect("dispatch should succeed")
            .expect("request should produce a response");

        assert_eq!(response["id"], serde_json::json!(1));
        assert_eq!(
            response["error"]["code"],
            serde_json::json!(CONNECTION_NOT_INITIALIZED_CODE)
        );
        assert!(response.get("result").is_none());
        assert!(state.sessions.read().await.is_empty());
    }

    #[tokio::test]
    async fn session_creation_rejects_malformed_mcp_server_field() {
        let request = Request {
            id: serde_json::json!(1),
            method: String::from("session/new"),
            params: Some(serde_json::json!({
                "cwd": "/tmp",
                "mcpServers": "bogus",
            })),
        };

        let mut state = ConnectionState::new();
        state.initialized = true;
        let model = Arc::new(InertModel);
        let (sender, _) = mpsc::channel(2);

        let response = dispatch_request(request, &mut state, &model, &sender)
            .await
            .expect("dispatch should succeed")
            .expect("should return a response with an error field");

        {
            let registry = state.sessions.read().await;
            assert_eq!(registry.len(), 0);
        }

        assert_eq!(response["id"], serde_json::json!(1));
        assert_eq!(
            response["error"]["code"],
            serde_json::json!(INVALID_PARAMS_CODE)
        );
        assert!(response.get("result").is_none());
    }

    #[tokio::test]
    async fn session_creation_rejects_relative_working_directory() {
        let request = Request {
            id: serde_json::json!(1),
            method: String::from("session/new"),
            params: Some(serde_json::json!({
                "cwd": "relative/path",
                "mcpServers": [],
            })),
        };

        let mut state = ConnectionState::new();
        state.initialized = true;

        let model = Arc::new(InertModel);
        let (sender, _) = mpsc::channel(2);

        let response = dispatch_request(request, &mut state, &model, &sender)
            .await
            .expect("dispatch should succeed")
            .expect("should return a response with an error field");

        {
            let registry = state.sessions.read().await;
            assert_eq!(registry.len(), 0);
        }

        assert_eq!(response["id"], serde_json::json!(1));
        assert_eq!(
            response["error"]["code"],
            serde_json::json!(INVALID_PARAMS_CODE)
        );
        assert!(response.get("result").is_none());
    }

    #[tokio::test]
    async fn session_creation_returns_distinct_independent_sessions() {
        let mut state = ConnectionState::new();
        state.initialized = true;

        fn make_request(request_id: u64, cwd: String) -> Request<Value> {
            Request {
                id: serde_json::json!(request_id),
                method: String::from("session/new"),
                params: Some(serde_json::json!({
                    "cwd": cwd,
                    "mcpServers": [],
                })),
            }
        }

        let first_request = make_request(1, "/tmp/session_one".to_string());
        let second_request = make_request(2, "/tmp/session_two".to_string());

        let model = Arc::new(InertModel);
        let (sender, _) = mpsc::channel(2);

        let first_response = dispatch_request(first_request, &mut state, &model, &sender)
            .await
            .expect("dispatch should succeed")
            .expect("request should produce a response");

        let second_response = dispatch_request(second_request, &mut state, &model, &sender)
            .await
            .expect("dispatch should succeed")
            .expect("request should produce a response");

        assert_eq!(first_response["id"], serde_json::json!(1));
        assert_eq!(second_response["id"], serde_json::json!(2));

        let first_session_id: SessionId =
            serde_json::from_value(first_response["result"]["sessionId"].clone())
                .expect("session ID should be a valid UUID");
        let second_session_id: SessionId =
            serde_json::from_value(second_response["result"]["sessionId"].clone())
                .expect("session ID should be a valid UUID");

        assert_ne!(first_session_id, second_session_id);

        let (first_session_entry, second_session_entry) = {
            let registry = state.sessions.read().await;

            assert_eq!(registry.len(), 2);

            (
                registry
                    .get(&first_session_id)
                    .cloned()
                    .expect("returned session ID should exist in the registry"),
                registry
                    .get(&second_session_id)
                    .cloned()
                    .expect("returned session ID should exist in the registry"),
            )
        };

        {
            let first_session = first_session_entry.session.lock().await;
            assert_eq!(
                first_session.cwd(),
                std::path::Path::new("/tmp/session_one")
            );
            assert!(first_response.get("error").is_none());
        }

        {
            let second_session = second_session_entry.session.lock().await;

            assert_eq!(
                second_session.cwd(),
                std::path::Path::new("/tmp/session_two")
            );
            assert!(second_response.get("error").is_none());
        }
    }

    #[tokio::test]
    async fn session_creation_returns_id_and_retains_working_directory() {
        let request = Request {
            id: serde_json::json!(1),
            method: String::from("session/new"),
            params: Some(serde_json::json!({
                "cwd": "/tmp",
                "mcpServers": [],
            })),
        };

        let mut state = ConnectionState::new();
        state.initialized = true;
        let model = Arc::new(InertModel);
        let (sender, _) = mpsc::channel(2);

        let response = dispatch_request(request, &mut state, &model, &sender)
            .await
            .expect("dispatch should succeed")
            .expect("request should produce a response");

        assert_eq!(response["id"], serde_json::json!(1));
        let session_id: SessionId = serde_json::from_value(response["result"]["sessionId"].clone())
            .expect("session ID should be a valid UUID");

        let session_entry = {
            let registry = state.sessions.read().await;

            registry
                .get(&session_id)
                .cloned()
                .expect("returned session ID should exist in the registry")
        };

        let session = session_entry.session.lock().await;

        assert_eq!(session.cwd(), std::path::Path::new("/tmp"));
        assert!(response.get("error").is_none());
    }

    #[tokio::test]
    async fn can_initialize_without_specifying_client_capabilities() {
        let request = serde_json::json!({
          "jsonrpc": "2.0",
          "id": 1,
          "method": "initialize",
          "params": {
              "protocolVersion": 1,
          }
        });

        let mut input = serde_json::to_string(&request).expect("request should serialize");
        input.push('\n');
        let reader = Cursor::new(input.into_bytes());
        let (mut client_end, agent_end) = duplex(1024);

        let agent_output = BufWriter::new(agent_end);

        run_acp(reader, agent_output)
            .await
            .expect("request should succeed");

        let mut received = Vec::new();
        client_end
            .read_to_end(&mut received)
            .await
            .expect("client should read agent output");

        let result = serde_json::from_str(&String::from_utf8(received).expect("to be a string"))
            .expect("to be a valid message");

        validate_version(&result).expect("response should use JSON-RPC 2.0");

        assert_eq!(result["id"], serde_json::json!(1));
        assert!(
            result.get("error").is_none(),
            "a success response must not contain an error"
        );
    }

    #[tokio::test]
    async fn session_prompt_request_rejected_with_malformed_prompt_parameters() {
        let mut state = ConnectionState::new();
        state.initialized = true;

        let value = serde_json::json!({
          "jsonrpc": "2.0",
          "id": 3,
          "method": "session/prompt",
          "params": {
              "sessionId": "00000000-0000-0000-0000-000000000001",
              "prompt": []
          }
        });
        let request: Request<Value> = serde_json::from_value(value).expect("to parse as JSON-RPC");
        let model = Arc::new(InertModel);
        let (sender, _receiver) = mpsc::channel(2);

        let result = dispatch_request(request, &mut state, &model, &sender)
            .await
            .expect("dispatch should succeed")
            .expect("request should produce an error response");

        assert_eq!(result["id"], serde_json::json!(3));
        assert!(
            result.get("result").is_none(),
            "an error response must not contain a result"
        );
        assert_eq!(result["error"]["code"], INVALID_PARAMS_CODE);
    }

    #[tokio::test]
    async fn session_prompt_request_rejected_with_missing_session() {
        let mut state = ConnectionState::new();
        state.initialized = true;

        let session_id = "00000000-0000-0000-0000-000000000001";
        let value = make_prompt_request(3, session_id, "What is 1+1?");
        let request: Request<Value> = serde_json::from_value(value).expect("to parse as JSON-RPC");
        let model = Arc::new(InertModel);
        let (sender, _receiver) = mpsc::channel(2);

        let result = dispatch_request(request, &mut state, &model, &sender)
            .await
            .expect("dispatch should succeed")
            .expect("request should produce an error response");

        assert_eq!(result["id"], serde_json::json!(3));
        assert!(
            result.get("result").is_none(),
            "an error response must not contain a result"
        );
        assert_eq!(result["error"]["code"], RESOURCE_NOT_FOUND_CODE);
    }

    #[tokio::test]
    async fn session_prompt_execution_does_not_block_reader() {
        let started = Arc::new(Notify::new());
        let released = Arc::new(Notify::new());
        let responses = [TimeGated {
            response: assistant_reply("scripted answer"),
            started: started.clone(),
            released: released.clone(),
        }];

        let model = ScriptedModel::new(VecDeque::from(responses));

        let (client, agent) = duplex(1024);
        let (agent_reader, agent_writer) = split(agent);
        let (client_reader, mut client_writer) = split(client);
        let reader = BufReader::new(agent_reader);

        let mut client_reader = BufReader::new(client_reader);

        let agent_task = tokio::spawn(run_acp_with_model(reader, agent_writer, model));

        initialize(&mut client_writer, &mut client_reader).await;
        let session_id = create_new_session(&mut client_writer, &mut client_reader, 2).await;

        let prompt_request = make_prompt_request(3, &session_id, "What is 1+1?");

        send_frame(&mut client_writer, &prompt_request).await;

        started.notified().await;

        let _ = create_new_session(&mut client_writer, &mut client_reader, 4).await;

        released.notify_one();

        let result = read_frame(&mut client_reader).await;

        assert_eq!(result["params"]["sessionId"], session_id);
        let expected_response = make_agent_message_chunk(&session_id, "scripted answer");

        assert_eq!(expected_response, result);

        let response = read_frame(&mut client_reader).await;
        assert_eq!(response["id"], 3);
        assert_eq!(response["result"]["stopReason"], "end_turn");
        assert!(response["error"].is_null());

        client_writer
            .shutdown()
            .await
            .expect("client writer to shutdown");

        agent_task
            .await
            .expect("to join successfully")
            .expect("to shutdown gracefully");
    }

    #[tokio::test]
    async fn session_prompt_runs_model_and_retains_conversation() {
        let responses = [Done {
            response: assistant_reply("scripted answer"),
        }];
        let model = ScriptedModel::new(VecDeque::from(responses));
        let session_id = "00000000-0000-0000-0000-000000000001";
        let session_entry = Arc::new(SessionEntry::new(Session::empty(PathBuf::from("/tmp"))));

        let mut state = ConnectionState::new();
        {
            let mut registry = state.sessions.write().await;
            let uuid = Uuid::parse_str(session_id).expect("to be a valid UUID");
            let session_id = SessionId(uuid);

            registry.insert(session_id, Arc::clone(&session_entry));
        }
        state.initialized = true;

        let session_id = "00000000-0000-0000-0000-000000000001";
        let value = make_prompt_request(3, session_id, "What is 1+1?");
        let request: Request<Value> = serde_json::from_value(value).expect("to parse as JSON-RPC");
        let model = Arc::new(model);
        let (sender, mut receiver) = mpsc::channel(2);

        let dispatch_result = dispatch_request(request, &mut state, &model, &sender)
            .await
            .expect("dispatch should succeed");

        drop(sender);

        assert!(
            dispatch_result.is_none(),
            "spawned prompt handling should send through the writer channel"
        );

        let update = receiver
            .recv()
            .await
            .expect("to receive an agent_updated_message");

        assert_eq!(update["jsonrpc"], "2.0");
        assert_eq!(update["method"], "session/update");
        assert!(update.get("id").is_none());
        assert_eq!(
            update["params"]["update"]["sessionUpdate"],
            "agent_message_chunk"
        );
        assert_eq!(update["params"]["update"]["content"]["type"], "text");
        assert_eq!(
            update["params"]["sessionId"],
            "00000000-0000-0000-0000-000000000001"
        );
        assert_eq!(
            update["params"]["update"]["content"]["text"],
            "scripted answer"
        );

        let result = receiver
            .recv()
            .await
            .expect("prompt should send a final response");

        assert_eq!(result["jsonrpc"], "2.0");
        assert_eq!(result["id"], serde_json::json!(3));
        assert!(
            result.get("error").is_none(),
            "a result response must not contain an error"
        );
        assert_eq!(result["result"]["stopReason"], "end_turn");

        {
            let guard = session_entry.active_turn_stop.lock().await;
            assert!(guard.is_none());
        }
        let session_guard = session_entry.session.lock().await;
        assert_eq!(session_guard.conversation.messages.len(), 2);

        assert_eq!(session_guard.conversation.messages[0]["role"], "user");
        assert_eq!(
            session_guard.conversation.messages[0]["content"],
            "What is 1+1?"
        );

        assert_eq!(session_guard.conversation.messages[1]["role"], "assistant");
        assert_eq!(
            session_guard.conversation.messages[1]["content"],
            "scripted answer"
        );
    }

    #[tokio::test]
    async fn should_cancel_active_prompts() {
        let started = Arc::new(Notify::new());
        let released = Arc::new(Notify::new());
        let responses = [
            Done {
                response: assistant_reply("first"),
            },
            TimeGated {
                response: assistant_reply("second"),
                started: started.clone(),
                released: released.clone(),
            },
            Done {
                response: assistant_reply("third"),
            },
        ];

        let (client_end, agent_end) = duplex(1024);
        let (client_read, mut client_writer) = tokio::io::split(client_end);
        let (agent_read, agent_write) = tokio::io::split(agent_end);

        let agent_input = tokio::io::BufReader::new(agent_read);
        let agent_output = BufWriter::new(agent_write);

        let mut client_reader = tokio::io::BufReader::new(client_read);

        let model = Arc::new(ScriptedModel::new(VecDeque::from(responses)));

        let model_clone = model.clone();
        let agent_task = tokio::spawn(async move {
            run_acp_with_model(agent_input, agent_output, model_clone).await
        });

        initialize(&mut client_writer, &mut client_reader).await;
        let session_id = create_new_session(&mut client_writer, &mut client_reader, 2).await;

        // Send a prompt and verify the scripted model started processing.
        let prompt_request = make_prompt_request(3, &session_id, "first question");

        send_frame(&mut client_writer, &prompt_request).await;
        let response = read_frame(&mut client_reader).await;

        assert_eq!(response["params"]["sessionId"], session_id);
        let expected_response = make_agent_message_chunk(&session_id, "first");

        assert_eq!(expected_response, response);

        let response = read_frame(&mut client_reader).await;
        assert_eq!(response["id"], 3);
        assert_eq!(response["result"]["stopReason"], "end_turn");
        assert!(response["error"].is_null());

        // Send the second message that we are going to cancel
        let second_prompt_request = make_prompt_request(4, &session_id, "second question");

        send_frame(&mut client_writer, &second_prompt_request).await;

        started.notified().await;

        let cancel_notification = make_cancel_notification(&session_id);

        send_frame(&mut client_writer, &cancel_notification).await;
        let result = read_frame(&mut client_reader).await;
        validate_version(&result).expect("response should use JSON-RPC 2.0");

        assert_eq!(result["id"], serde_json::json!(4));
        assert!(
            result.get("error").is_none(),
            "a success response must not contain an error"
        );
        assert_eq!(result["result"]["stopReason"], "cancelled");

        // Send the third prompt and check the history doesn't include the second cancelled prompt.
        let third_prompt_request = make_prompt_request(5, &session_id, "third question");

        send_frame(&mut client_writer, &third_prompt_request).await;
        let response = read_frame(&mut client_reader).await;

        assert_eq!(response["params"]["sessionId"], session_id);
        let expected_response = make_agent_message_chunk(&session_id, "third");

        assert_eq!(expected_response, response);
        let response = read_frame(&mut client_reader).await;

        assert_eq!(response["id"], 5);
        assert_eq!(response["result"]["stopReason"], "end_turn");
        assert!(response["error"].is_null());

        let (num_messages, third_request) = {
            let guard = model.requests.lock().unwrap();
            (guard.len(), guard.get(2).unwrap().clone())
        };

        assert_eq!(3, num_messages);
        assert_eq!(
            third_request["messages"],
            serde_json::json!([
                {"role": "user", "content": "first question"},
                {"role": "assistant", "content": "first", "tool_calls": null},
                {"role": "user", "content": "third question"}
            ])
        );

        client_writer
            .shutdown()
            .await
            .expect("to cleanly shutdown the writer");

        agent_task
            .await
            .expect("agent task should join")
            .expect("ACP transport should shutdown cleanly");
    }

    #[tokio::test]
    async fn notification_produces_no_response() {
        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "initialized"
        });

        let mut frame = serde_json::to_vec(&notification).expect("notification should serialize");
        frame.push(b'\n');

        let input = Cursor::new(frame);
        let (mut client_output, agent_output) = duplex(1024);

        run_acp(input, agent_output)
            .await
            .expect("notification should be processed through clean EOF");

        let mut received = Vec::new();
        client_output
            .read_to_end(&mut received)
            .await
            .expect("client should read agent output");

        assert!(
            received.is_empty(),
            "notifications must not receive a response"
        );
    }

    #[tokio::test]
    async fn invalid_jsonrpc_notification() {
        let notification = serde_json::json!({
            "jsonrpc": "1.0",
            "method": "initialized"
        });

        let mut frame = serde_json::to_vec(&notification).expect("notification should serialize");
        frame.push(b'\n');

        let input = Cursor::new(frame);
        let output = tokio::io::sink();

        let error = run_acp(input, output)
            .await
            .expect_err("notification should be rejected");

        assert_eq!(error.to_string(), "expected JSON-RPC version 2.0")
    }

    #[tokio::test]
    async fn invalid_jsonrpc_response() {
        let response = serde_json::json!({
            "jsonrpc": "1.0",
            "id": "1",
            "error": {}
        });

        let mut frame = serde_json::to_vec(&response).expect("response should serialize");
        frame.push(b'\n');

        let input = Cursor::new(frame);
        let output = tokio::io::sink();

        let error = run_acp(input, output)
            .await
            .expect_err("response should be rejected");

        assert_eq!(error.to_string(), "expected JSON-RPC version 2.0")
    }

    #[test]
    fn classify_request() {
        let value = serde_json::json!({
            "id": 1,
            "method": "initialize",
            "params": {},
        });

        let kind = classify_message(&value).expect("to be a valid Request");

        assert_eq!(kind, MessageKind::Request);
    }

    #[test]
    fn classify_notification() {
        let value = serde_json::json!({
            "method": "method_name",
            "params": {},
        });

        let kind = classify_message(&value).expect("to be a valid Notification");

        assert_eq!(kind, MessageKind::Notification);
    }

    #[test]
    fn classify_successful_response() {
        let value = serde_json::json!({
            "id": 1,
            "result": {}
        });

        let kind = classify_message(&value).expect("to be a valid Response");

        assert_eq!(kind, MessageKind::Response);
    }

    #[test]
    fn classify_error_response() {
        let value = serde_json::json!({
            "id": 1,
            "error": {}
        });

        let kind = classify_message(&value).expect("to be a valid Response");

        assert_eq!(kind, MessageKind::Response);
    }

    #[test]
    fn classify_result_and_error_as_invalid() {
        let value = serde_json::json!({
            "id": 1,
            "error": {},
            "result": {}
        });

        let error = classify_message(&value)
            .expect_err("a message with a result and an error should be rejected");

        assert_eq!(error.to_string(), "invalid JSON-RPC message shape");
    }

    #[test]
    fn classify_method_and_result_as_invalid() {
        let value = serde_json::json!({
            "id": 1,
            "method": "message",
            "result": {}
        });

        let error = classify_message(&value)
            .expect_err("a message with a method and a result should be rejected");

        assert_eq!(error.to_string(), "invalid JSON-RPC message shape");
    }

    #[test]
    fn classify_method_and_error_as_invalid() {
        let value = serde_json::json!({
            "id": 1,
            "method": "message",
            "error": {}
        });

        let error = classify_message(&value)
            .expect_err("a message with a method and an error should be rejected");

        assert_eq!(error.to_string(), "invalid JSON-RPC message shape");
    }

    #[test]
    fn classify_result_without_id_as_invalid() {
        let value = serde_json::json!({
            "result": {}
        });

        let error = classify_message(&value)
            .expect_err("a response-shaped message without an ID should be rejected");

        assert_eq!(error.to_string(), "invalid JSON-RPC message shape");
    }

    #[test]
    fn classify_empty_object_as_invalid() {
        let value = serde_json::json!({});

        let error = classify_message(&value)
            .expect_err("an object without identifying fields should be rejected");

        assert_eq!(error.to_string(), "invalid JSON-RPC message shape");
    }

    #[test]
    fn classify_non_object_as_invalid() {
        let value = serde_json::json!([]);

        let error = classify_message(&value).expect_err("a JSON-RPC message must be an object");

        assert_eq!(error.to_string(), "JSON-RPC message must be an object.");
    }

    #[tokio::test]
    async fn writes_one_compact_newline_terminated_frame_and_flushes() {
        let value = serde_json::json!({
            "result": {
                "protocolVersion": 1
            }
        });

        let (mut client_end, agent_end) = duplex(1024);

        let mut agent_output = BufWriter::new(agent_end);

        write_frame(&mut agent_output, &value)
            .await
            .expect("frame should be written successfully.");
        drop(agent_output);

        let mut received = String::new();
        client_end
            .read_to_string(&mut received)
            .await
            .expect("client should read the frame");

        assert_eq!(received, "{\"result\":{\"protocolVersion\":1}}\n");
    }

    fn valid_request_frame_with_size(size: usize) -> String {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 0,
            "method": "initialize",
        });
        let mut frame = serde_json::to_string(&request).expect("request should serialize");
        let padding_bytes = size
            .checked_sub(frame.len() + 1)
            .expect("target size should fit the request and newline");

        frame.push_str(&" ".repeat(padding_bytes));
        frame.push('\n');
        assert_eq!(frame.len(), size, "fixture should have the target size");
        frame
    }

    #[tokio::test]
    async fn clean_eof_exits_successfully() {
        let reader = Cursor::new(b"");
        let writer = tokio::io::sink();

        run_acp(reader, writer)
            .await
            .expect("clean EOF should end ACP mode successfully");
    }

    #[test]
    fn invalidates_jsonrpc_other_than_2() {
        let request = serde_json::json!({
            "jsonrpc": "1.0",
            "id": 0,
            "method": "initialize",
        });

        let error = validate_version(&request)
            .expect_err("JSON-RPC versions other than 2.0 should be rejected");

        assert_eq!(error.to_string(), "expected JSON-RPC version 2.0");
    }

    #[tokio::test]
    async fn frame_at_maximum_size_is_accepted() {
        let input = valid_request_frame_with_size(MAX_FRAME_BYTES);
        let reader = Cursor::new(input.into_bytes());
        let writer = tokio::io::sink();

        run_acp(reader, writer)
            .await
            .expect("a frame at the maximum size should be accepted");
    }

    #[tokio::test]
    async fn frame_over_maximum_size_is_rejected() {
        let input = valid_request_frame_with_size(MAX_FRAME_BYTES + 1);
        let reader = Cursor::new(input.into_bytes());
        let writer = tokio::io::sink();

        let error = run_acp(reader, writer)
            .await
            .expect_err("an oversized frame should be rejected");
        let io_error = error
            .downcast_ref::<std::io::Error>()
            .expect("error should be an io::Error");

        assert_eq!(io_error.kind(), std::io::ErrorKind::InvalidData);
    }

    #[tokio::test]
    async fn the_last_frame_must_have_a_newline() {
        let messages = serde_json::json!([{
          "jsonrpc": "2.0",
          "id": 0,
          "method": "first",
        },{
          "jsonrpc": "2.0",
          "id": 1,
          "method": "second",
        }]);

        let input = messages
            .as_array()
            .expect("messages fixture should be an array")
            .iter()
            .map(|message| serde_json::to_string(&message).expect("message should serialize"))
            .collect::<Vec<_>>()
            .join("\n");

        let cursor = Cursor::new(input.into_bytes());
        let writer = tokio::io::sink();
        let error = run_acp(cursor, writer)
            .await
            .expect_err("unterminated frame should be rejected");

        let io_error = error
            .downcast_ref::<std::io::Error>()
            .expect("error to be an io::Error");
        assert_eq!(io_error.kind(), std::io::ErrorKind::UnexpectedEof);
    }

    #[tokio::test]
    async fn runs_until_eof() {
        let messages = serde_json::json!([{
          "jsonrpc": "2.0",
          "id": 0,
          "method": "first",
        },{
          "jsonrpc": "2.0",
          "id": 1,
          "method": "second",
        }]);

        let input = messages
            .as_array()
            .expect("messages fixture should be an array")
            .iter()
            .map(|message| serde_json::to_string(&message).expect("message should serialize"))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";

        let cursor = Cursor::new(input.into_bytes());
        let (mut client_end, agent_end) = duplex(1024);
        let writer = BufWriter::new(agent_end);

        run_acp(cursor, writer)
            .await
            .expect("to read both messages");

        let mut received = String::new();
        client_end
            .read_to_string(&mut received)
            .await
            .expect("client should read agent output");

        let responses = received
            .lines()
            .map(|line| {
                serde_json::from_str::<serde_json::Value>(line)
                    .expect("each response should be valid JSON")
            })
            .collect::<Vec<_>>();

        assert_eq!(responses.len(), 2);
        assert_eq!(responses[0]["id"], serde_json::json!(0));
        assert_eq!(responses[1]["id"], serde_json::json!(1));
    }

    #[tokio::test]
    async fn respond_with_version_and_capabilities() {
        let init_request = serde_json::json!({
          "jsonrpc": "2.0",
          "id": 0,
          "method": "initialize",
          "params": {
            "protocolVersion": 1,
            "clientCapabilities": {
              "fs": {
                "readTextFile": true,
                "writeTextFile": true
              },
              "terminal": true
            },
            "clientInfo": {
              "name": "my-client",
              "title": "My Client",
              "version": "1.0.0"
            }
          }
        });

        let mut input =
            serde_json::to_string(&init_request).expect("initialize message should serialize");
        input.push('\n');
        let reader = Cursor::new(input.into_bytes());
        let (mut client_end, agent_end) = duplex(1024);

        let agent_output = BufWriter::new(agent_end);

        run_acp(reader, agent_output)
            .await
            .expect("initialize request should succeed");

        let mut received = Vec::new();
        client_end
            .read_to_end(&mut received)
            .await
            .expect("client should read agent output");

        let result = serde_json::from_str(&String::from_utf8(received).expect("to be a string"))
            .expect("to be a valid message");

        validate_version(&result).expect("response should use JSON-RPC 2.0");

        assert_eq!(result["id"], serde_json::json!(0));
        assert_eq!(result["result"]["protocolVersion"], serde_json::json!(1));
        assert!(result["result"]["agentCapabilities"].is_object());
        assert_eq!(result["result"]["authMethods"], serde_json::json!([]));
        assert!(
            result.get("error").is_none(),
            "a successful response must not contain an error"
        );
    }

    #[tokio::test]
    async fn respond_to_unknown_methods() {
        let bad_request = serde_json::json!({
          "jsonrpc": "2.0",
          "id": 2,
          "method": "this is not real",
          "params": {}
        });

        let (client, agent) = duplex(1024);
        let (agent_reader, agent_writer) = split(agent);
        let (client_reader, mut client_writer) = split(client);

        let agent_reader = BufReader::new(agent_reader);
        let mut client_reader = BufReader::new(client_reader);

        let model = ScriptedModel::new(VecDeque::new());

        let agent_task = tokio::spawn(run_acp_with_model(agent_reader, agent_writer, model));
        initialize(&mut client_writer, &mut client_reader).await;

        send_frame(&mut client_writer, &bad_request).await;
        let result = read_frame(&mut client_reader).await;

        validate_version(&result).expect("response should use JSON-RPC 2.0");

        assert_eq!(result["id"], serde_json::json!(2));
        assert!(
            result.get("result").is_none(),
            "an error response must not contain a result"
        );

        let error = &result["error"];
        assert_eq!(error["code"], METHOD_NOT_FOUND_CODE);
        assert_eq!(error["message"], "Method not found");

        client_writer
            .shutdown()
            .await
            .expect("client writer to shutdown");

        agent_task
            .await
            .expect("to join successfully")
            .expect("to shutdown gracefully");
    }

    #[tokio::test]
    async fn responds_with_invalid_params_when_client_info_is_missing_name() {
        let request = make_initialize();

        let mut input = serde_json::to_string(&request).expect("request should serialize");
        input.push('\n');
        let reader = Cursor::new(input.into_bytes());
        let (mut client_end, agent_end) = duplex(1024);

        let agent_output = BufWriter::new(agent_end);

        run_acp(reader, agent_output)
            .await
            .expect("request should succeed");

        let mut received = Vec::new();
        client_end
            .read_to_end(&mut received)
            .await
            .expect("client should read agent output");

        let result = serde_json::from_str(&String::from_utf8(received).expect("to be a string"))
            .expect("to be a valid message");

        validate_version(&result).expect("response should use JSON-RPC 2.0");

        assert_eq!(result["id"], serde_json::json!(1));
        assert!(
            result.get("result").is_none(),
            "an error response must not contain a result"
        );

        let error = &result["error"];
        assert_eq!(error["code"], INVALID_PARAMS_CODE);
        assert_eq!(error["message"], "Invalid params")
    }

    #[tokio::test]
    async fn writer_can_flush_while_reader_remains_blocked() {
        let request = serde_json::json!({
          "jsonrpc": "2.0",
          "id": 1,
          "method": "initialize",
          "params": {
              "protocolVersion": 1,
          }
        });

        let mut input = serde_json::to_string(&request).expect("request should serialize");
        input.push('\n');

        let (client_end, agent_end) = duplex(1024);
        let (client_read, mut client_writer) = tokio::io::split(client_end);
        let (agent_read, agent_write) = tokio::io::split(agent_end);

        let agent_input = tokio::io::BufReader::new(agent_read);
        let agent_output = BufWriter::new(agent_write);

        let mut client_reader = tokio::io::BufReader::new(client_read);

        let agent_task = tokio::spawn(run_acp(agent_input, agent_output));

        client_writer
            .write_all(input.as_bytes())
            .await
            .expect("to write the initialize message");

        let mut response = String::new();
        client_reader
            .read_line(&mut response)
            .await
            .expect("to read the agent's response");

        let response: serde_json::Value =
            serde_json::from_str(&response).expect("agent response should be valid JSON");

        validate_version(&response).expect("response should use JSON-RPC 2.0");
        assert_eq!(response["id"], serde_json::json!(1));
        assert_eq!(response["result"]["protocolVersion"], serde_json::json!(1));

        client_writer
            .shutdown()
            .await
            .expect("to cleanly shutdown the writer");

        agent_task
            .await
            .expect("agent task should join")
            .expect("ACP transport should shutdown cleanly");
    }

    #[tokio::test]
    async fn writer_drains_response_after_client_input_reaches_eof() {
        let request = serde_json::json!({
          "jsonrpc": "2.0",
          "id": 1,
          "method": "initialize",
          "params": {
              "protocolVersion": 1,
          }
        });

        let mut input = serde_json::to_string(&request).expect("request should serialize");
        input.push('\n');

        let (client_end, agent_end) = duplex(1024);
        let (client_read, mut client_writer) = tokio::io::split(client_end);
        let (agent_read, agent_write) = tokio::io::split(agent_end);

        let agent_input = tokio::io::BufReader::new(agent_read);
        let agent_output = BufWriter::new(agent_write);

        let mut client_reader = tokio::io::BufReader::new(client_read);

        let agent_task = tokio::spawn(run_acp(agent_input, agent_output));

        client_writer
            .write_all(input.as_bytes())
            .await
            .expect("client to write the initialize message");

        client_writer
            .shutdown()
            .await
            .expect("to cleanly shutdown the writer");

        let mut response = String::new();
        client_reader
            .read_line(&mut response)
            .await
            .expect("to read the agent's response");

        let response: serde_json::Value =
            serde_json::from_str(&response).expect("agent response should be valid JSON");

        validate_version(&response).expect("response should use JSON-RPC 2.0");
        assert_eq!(response["id"], serde_json::json!(1));
        assert_eq!(response["result"]["protocolVersion"], serde_json::json!(1));

        agent_task
            .await
            .expect("agent task should join")
            .expect("ACP transport should shutdown cleanly");
    }

    #[test]
    fn join_text_blocks_joins_text_with_a_newline() {
        let blocks = vec![
            PromptContentBlock::Text {
                text: "first".to_string(),
            },
            PromptContentBlock::Text {
                text: "second".to_string(),
            },
        ];

        let joined = join_text_blocks(blocks).expect("one joined text");
        assert_eq!("first\nsecond".to_string(), joined);
    }

    #[test]
    fn join_text_blocks_rejects_unsupported_blocks() {
        let blocks = vec![
            PromptContentBlock::Text {
                text: "text".to_string(),
            },
            PromptContentBlock::Unsupported,
        ];

        let joined = join_text_blocks(blocks);
        assert_eq!(None, joined);
    }

    #[test]
    fn parse_prompt_params_when_the_structure_is_valid() {
        let expected = "00000000-0000-0000-0000-000000000001";
        let params = serde_json::json!({
            "sessionId": &expected.to_string(),
            "prompt": [
                { "type": "text", "text": "Hello"}
            ]
        });

        let (session_id, prompt) =
            parse_prompt_params(Some(params)).expect("valid prompt params should parse");
        assert_eq!(
            session_id.0,
            Uuid::parse_str(expected).expect("expected ID to be a valid UUID")
        );

        assert_eq!(prompt, "Hello");
    }

    #[test]
    fn parse_prompt_params_rejects_unsupported_blocks() {
        let expected = "00000000-0000-0000-0000-000000000001";
        let params = serde_json::json!({
            "sessionId": &expected.to_string(),
            "prompt": [
                { "type": "image" }
            ]
        });

        parse_prompt_params(Some(params)).expect_err("unsupported blocks should not parse");
    }

    #[test]
    fn parse_prompt_params_rejects_malformed_parameters() {
        let params = serde_json::json!({
            "prompt": [
                { "type": "text", "text": "Hello" }
            ]
        });

        parse_prompt_params(Some(params)).expect_err("malformed parameters should not parse");
    }

    #[test]
    fn parse_prompt_params_rejects_empty_prompt_array() {
        let expected = "00000000-0000-0000-0000-000000000001";
        let params = serde_json::json!({
            "sessionId": expected.to_string(),
            "prompt": []
        });

        parse_prompt_params(Some(params)).expect_err("empty prompt arrays should not parse");
    }

    #[tokio::test]
    async fn get_session_should_return_sessions_if_they_are_registered() {
        let session = Session::empty(PathBuf::from("/tmp"));
        let session_entry = Arc::new(SessionEntry::new(session));
        let mut session_by_id = HashMap::new();
        let session_id = SessionId(
            Uuid::parse_str("00000000-0000-0000-0000-000000000001").expect("a valid UUID"),
        );

        session_by_id.insert(session_id.clone(), Arc::clone(&session_entry));
        let registry = Arc::new(RwLock::new(session_by_id));

        let handle = get_session(&registry, &session_id)
            .await
            .expect("a concrete session");

        assert!(Arc::ptr_eq(&session_entry, &handle));
    }

    #[tokio::test]
    async fn get_session_should_return_none_for_absent_sessions() {
        let session = Session::empty(PathBuf::from("/tmp"));

        let session_entry = Arc::new(SessionEntry::new(session));
        let mut session_by_id = HashMap::new();
        let first_session_id = SessionId(
            Uuid::parse_str("00000000-0000-0000-0000-000000000001").expect("a valid UUID"),
        );
        let missing_session_id = SessionId(
            Uuid::parse_str("00000000-0000-0000-0000-000000000002").expect("a valid UUID"),
        );

        session_by_id.insert(first_session_id.clone(), session_entry);
        let registry = Arc::new(RwLock::new(session_by_id));

        assert!(get_session(&registry, &missing_session_id).await.is_none())
    }
}
