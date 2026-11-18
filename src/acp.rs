mod errors;
mod json_rpc;
mod message_handling;
mod schema;
mod session_registry;
mod transport;

#[cfg(test)]
mod test_client;

use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;

use serde_json::Value;

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite};

use tokio::sync::{RwLock, mpsc};

use crate::acp::json_rpc::{
    ErrorResponse, MessageKind, Notification, Request, classify_message, validate_version,
};
use crate::acp::message_handling::{dispatch_request, handle_notification};
use crate::acp::session_registry::SessionRegistry;
use crate::acp::transport::write_frames;
use crate::agent::{Model, OpenRouterModel};

type AcpError = Box<dyn Error + Send + Sync>;

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

const MAX_FRAME_BYTES: usize = 64 * 1024;

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
                let request_id = request.id.clone();
                validate_request(&request)?;

                match dispatch_request(request, &mut state, &model, &sender).await {
                    Ok(maybe_value) => {
                        if let Some(value) = maybe_value {
                            sender.send(value).await?;
                        }
                    }
                    Err(error) => {
                        let response = ErrorResponse::from(error, request_id);
                        sender.send(serde_json::to_value(response)?).await?;
                    }
                }
            }
            MessageKind::Notification => {
                let notification: Notification<Value> = serde_json::from_str(&input)?;
                handle_notification(notification, &mut state).await;
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

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, io::Cursor, sync::Arc, time::Duration};

    use serde_json::Value;
    use tokio::{
        fs,
        io::{
            AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, BufWriter, DuplexStream,
            ReadHalf, duplex, split,
        },
        sync::Notify,
        time::{sleep, timeout},
    };

    use crate::{
        acp::{
            MAX_FRAME_BYTES, Request,
            json_rpc::{INVALID_PARAMS_CODE, METHOD_NOT_FOUND_CODE},
            run_acp, run_acp_with_model,
            schema::InitializeParams,
            test_client::TestClient,
            validate_version,
        },
        scripted_model::{
            ScriptedModel,
            TurnResponse::{Done, TimeGated},
        },
        test_messages::{
            assistant_reply, assistant_reply_with_tool_calls, make_agent_message_chunk,
            make_initialize, make_tool_call,
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
    async fn session_prompt_execution_does_not_block_reader() {
        let started = Arc::new(Notify::new());
        let released = Arc::new(Notify::new());
        let responses = [TimeGated {
            response: assistant_reply("scripted answer"),
            started: started.clone(),
            released: released.clone(),
        }];

        let model = ScriptedModel::new(VecDeque::from(responses));

        let mut client = TestClient::new();
        let agent_end = client.agent_end.take().unwrap();

        let (agent_reader, agent_writer) = split(agent_end);

        let reader = BufReader::new(agent_reader);

        let agent_task = tokio::spawn(run_acp_with_model(reader, agent_writer, model));

        client.initialize().await;
        let session_id = client.create_new_session().await;
        let request_id = client.prompt(&session_id, "What is 1+1?").await;

        started.notified().await;

        let _ = client.create_new_session().await;

        released.notify_one();

        let result = client.read_frame().await;

        assert_eq!(result["params"]["sessionId"], session_id);
        let expected_response = make_agent_message_chunk(&session_id, "scripted answer");

        assert_eq!(expected_response, result);

        let response = client.read_frame().await;
        assert_eq!(response["id"], request_id);
        assert_eq!(response["result"]["stopReason"], "end_turn");
        assert!(response["error"].is_null());

        client.shutdown().await;

        agent_task
            .await
            .expect("to join successfully")
            .expect("to shutdown gracefully");
    }

    #[tokio::test]
    async fn cancel_active_prompts() {
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
        let model = Arc::new(ScriptedModel::new(VecDeque::from(responses)));

        let mut client = TestClient::new();

        let agent_end = client.agent_end.take().unwrap();
        let (agent_read, agent_write) = tokio::io::split(agent_end);

        let agent_input = tokio::io::BufReader::new(agent_read);
        let agent_output = BufWriter::new(agent_write);

        let model_clone = model.clone();
        let agent_task = tokio::spawn(async move {
            run_acp_with_model(agent_input, agent_output, model_clone).await
        });

        client.initialize().await;
        let session_id = client.create_new_session().await;

        let first_question_request_id = client.prompt(&session_id, "first question").await;

        let response = client.read_frame().await;
        assert_eq!(response["params"]["sessionId"], session_id);
        let expected_response = make_agent_message_chunk(&session_id, "first");

        assert_eq!(expected_response, response);

        let response = client.read_frame().await;
        assert_eq!(response["id"], first_question_request_id);
        assert_eq!(response["result"]["stopReason"], "end_turn");
        assert!(response["error"].is_null());

        // Send the second message that we are going to cancel
        let second_prompt_request_id = client.prompt(&session_id, "second question").await;
        started.notified().await;

        client.cancel(&session_id).await;
        let result = client.read_frame().await;
        validate_version(&result).expect("response should use JSON-RPC 2.0");

        assert_eq!(result["id"], serde_json::json!(second_prompt_request_id));
        assert!(
            result.get("error").is_none(),
            "a success response must not contain an error"
        );
        assert_eq!(result["result"]["stopReason"], "cancelled");

        // Send the third prompt and check the history doesn't include the second cancelled prompt.
        let third_question_request_id = client.prompt(&session_id, "third question").await;

        let response = client.read_frame().await;
        assert_eq!(response["params"]["sessionId"], session_id);
        let expected_response = make_agent_message_chunk(&session_id, "third");

        assert_eq!(expected_response, response);
        let response = read_frame(&mut client.client_reader).await;

        assert_eq!(response["id"], third_question_request_id);
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

        client.shutdown().await;

        agent_task
            .await
            .expect("agent task should join")
            .expect("ACP transport should shutdown cleanly");
    }

    #[tokio::test]
    async fn cancel_ignores_requests_for_unknown_and_idle_sessions() {
        let responses = [Done {
            response: assistant_reply("scripted answer"),
        }];
        let model = Arc::new(ScriptedModel::new(VecDeque::from(responses)));

        let mut client = TestClient::new();

        let agent_end = client.agent_end.take().unwrap();
        let (agent_read, agent_write) = tokio::io::split(agent_end);

        let agent_input = tokio::io::BufReader::new(agent_read);
        let agent_output = BufWriter::new(agent_write);

        let model_clone = model.clone();
        let agent_task = tokio::spawn(async move {
            run_acp_with_model(agent_input, agent_output, model_clone).await
        });

        client.initialize().await;
        let session_id = client.create_new_session().await;
        client.cancel(&session_id).await;

        let bogus_session_id = "00000000-0000-0000-0000-000000000001";
        client.cancel(bogus_session_id).await;

        assert_ne!(session_id, bogus_session_id);

        let prompt_request_id = client.prompt(&session_id, "What is 1+1?").await;

        let response = client.read_frame().await;
        assert_eq!(response["params"]["sessionId"], session_id);
        let expected_response = make_agent_message_chunk(&session_id, "scripted answer");

        assert_eq!(expected_response, response);
        let response = read_frame(&mut client.client_reader).await;

        assert_eq!(response["id"], prompt_request_id);
        assert_eq!(response["result"]["stopReason"], "end_turn");
        assert!(response["error"].is_null());

        client.shutdown().await;

        agent_task
            .await
            .expect("agent task should join")
            .expect("ACP transport should shutdown cleanly");
    }

    async fn wait_for_pid(filename: &str) -> u32 {
        timeout(Duration::from_secs(10), async {
            loop {
                match fs::read_to_string(filename).await {
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => panic!("Unable to read the PID file: {error}"),
                    Ok(pid) => {
                        if let Ok(pid) = pid.trim().parse::<u32>() {
                            return pid;
                        }
                    }
                }
                sleep(Duration::from_millis(250)).await;
            }
        })
        .await
        .expect("tool should write a valid PID within 10 seconds")
    }

    async fn wait_for_process_to_die(pid: u32) {
        timeout(Duration::from_secs(1), async {
            loop {
                let status = tokio::process::Command::new("kill")
                    .arg("-0")
                    .arg(pid.to_string())
                    .status()
                    .await
                    .expect("to issue the kill command");

                if !status.success() {
                    return;
                }

                sleep(Duration::from_millis(250)).await;
            }
        })
        .await
        .expect("for the process to be gone");
    }

    #[tokio::test]
    async fn prompt_cancelation_stops_tools() {
        let pid_filename = "prompt_cancelation_stops_tools.pid";

        match fs::remove_file(pid_filename).await {
            Ok(_) => (),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(_) => panic!("Unable to remove the old PID file."),
        }

        let args = format!("{{\"command\": \"echo $$ > {pid_filename}; exec sleep 5\"}}");
        let sleep_call = make_tool_call("sleep_call_id", "Bash", args.as_str());

        let responses = [Done {
            response: assistant_reply_with_tool_calls("first", vec![sleep_call]),
        }];
        let model = Arc::new(ScriptedModel::new(VecDeque::from(responses)));

        let mut client = TestClient::new();

        let agent_end = client.agent_end.take().unwrap();
        let (agent_read, agent_write) = tokio::io::split(agent_end);

        let agent_input = tokio::io::BufReader::new(agent_read);
        let agent_output = BufWriter::new(agent_write);

        let model_clone = model.clone();
        let agent_task = tokio::spawn(async move {
            run_acp_with_model(agent_input, agent_output, model_clone).await
        });

        client.initialize().await;
        let session_id = client.create_new_session().await;

        let prompt_request_id = client.prompt(&session_id, "first question").await;

        let pid = wait_for_pid(pid_filename).await;

        client.cancel(&session_id).await;

        let response = client.read_frame().await;
        assert_eq!(response["id"], prompt_request_id);
        assert_eq!(response["result"]["stopReason"], "cancelled");
        assert!(response["error"].is_null());

        wait_for_process_to_die(pid).await;

        client.shutdown().await;

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

        let mut client = TestClient::new();
        let (agent_reader, agent_writer) = split(client.agent_end.take().unwrap());

        let agent_reader = BufReader::new(agent_reader);

        let model = ScriptedModel::new(VecDeque::new());

        let agent_task = tokio::spawn(run_acp_with_model(agent_reader, agent_writer, model));
        client.initialize().await;

        client.send_frame(&bad_request).await;
        let result = client.read_frame().await;

        validate_version(&result).expect("response should use JSON-RPC 2.0");

        assert_eq!(result["id"], serde_json::json!(2));
        assert!(
            result.get("result").is_none(),
            "an error response must not contain a result"
        );

        let error = &result["error"];
        assert_eq!(error["code"], METHOD_NOT_FOUND_CODE);
        assert_eq!(error["message"], "Method not found");

        client.shutdown().await;

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
}
