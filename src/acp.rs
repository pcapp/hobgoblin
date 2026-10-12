use std::error::Error;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;

type AcpError = Box<dyn Error + Send + Sync>;

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

#[derive(Serialize)]
struct SuccessResponse<T> {
    jsonrpc: String,
    id: Value,
    result: T,
}

#[derive(Serialize)]
struct ErrorResponse {
    jsonrpc: String,
    id: Value,
    error: ResponseError,
}

#[derive(Serialize)]
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

async fn read_frames<R: AsyncBufRead + Unpin>(
    mut reader: R,
    sender: mpsc::Sender<Value>,
) -> Result<(), AcpError> {
    let mut input = String::new();

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
                if let Some(response) = dispatch(request).await? {
                    sender.send(response).await?;
                }
            }
            MessageKind::Notification | MessageKind::Response => {}
        }
    }

    Ok(())
}

pub async fn run_acp<R, W>(reader: R, writer: W) -> Result<(), AcpError>
where
    R: AsyncBufRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let (sender, receiver) = mpsc::channel(1);

    let mut reader_task = tokio::spawn(read_frames(reader, sender));
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

async fn dispatch(request: Request<Value>) -> Result<Option<Value>, AcpError> {
    if request.method == "initialize" {
        let params = request.params.unwrap_or(Value::Null);

        if serde_json::from_value::<InitializeParams>(params).is_err() {
            let response = ErrorResponse {
                jsonrpc: String::from("2.0"),
                id: request.id,
                error: ResponseError {
                    code: -32602,
                    message: String::from("Invalid params"),
                },
            };

            let response = serde_json::to_value(response)?;

            return Ok(Some(response));
        }

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

        let response = serde_json::to_value(response)?;
        Ok(Some(response))
    } else {
        let response = ErrorResponse {
            jsonrpc: String::from("2.0"),
            id: request.id,
            error: ResponseError {
                code: -32601,
                message: String::from("Method not found"),
            },
        };
        let response = serde_json::to_value(response)?;
        Ok(Some(response))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufWriter, duplex};

    use crate::acp::{
        InitializeParams, MAX_FRAME_BYTES, MessageKind, Request, classify_message, run_acp,
        validate_version, write_frame,
    };

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
        let request = serde_json::json!({
          "jsonrpc": "2.0",
          "id": 1,
          "method": "this is not real",
          "params": {}
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
        assert_eq!(error["code"], -32601);
        assert_eq!(error["message"], "Method not found")
    }

    #[tokio::test]
    async fn responds_with_invalid_params_when_client_info_is_missing_name() {
        let request = serde_json::json!({
          "jsonrpc": "2.0",
          "id": 1,
          "method": "initialize",
          "params": {
              "protocolVersion": 1,
              "clientCapabilities": {},
              "clientInfo": {
                  "version": "1.0.0"
              }
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
