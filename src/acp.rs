use serde::Deserialize;
use serde_json::Value;

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[derive(Deserialize)]
struct Request<T> {
    jsonrpc: String,
    id: Value,

    #[allow(dead_code)]
    method: String,

    #[allow(dead_code)]
    params: Option<T>,
}

#[derive(Debug, PartialEq, Eq)]
enum MessageKind {
    Request,
    Notification,
    Response,
}

const MAX_FRAME_BYTES: usize = 64 * 1024;

fn classify_message(value: &Value) -> Result<MessageKind, Box<dyn std::error::Error>> {
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

fn validate_request(request: &Request<Value>) -> Result<(), Box<dyn std::error::Error>> {
    if request.jsonrpc != "2.0" {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "expected JSON-RPC version 2.0",
        )
        .into());
    }

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

async fn write_frame<W: AsyncWrite + Unpin>(
    writer: &mut W,
    value: &Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut frame = serde_json::to_vec(value)?;
    frame.push(b'\n');

    writer.write_all(&frame).await?;
    writer.flush().await?;

    Ok(())
}

pub async fn run_acp<R: AsyncBufRead + Unpin, W: AsyncWrite + Unpin>(
    reader: &mut R,
    _writer: &mut W,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut input = String::new();

    loop {
        input.clear();
        let n = {
            let mut limited_reader = (&mut *reader).take((MAX_FRAME_BYTES + 1) as u64);

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
        match classify_message(&message)? {
            MessageKind::Request => {
                let request: Request<Value> = serde_json::from_str(&input)?;
                validate_request(&request)?;
            }
            MessageKind::Response | MessageKind::Notification => {}
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use serde_json::Value;
    use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, BufWriter, duplex};

    use crate::acp::{
        MAX_FRAME_BYTES, MessageKind, Request, classify_message, run_acp, validate_request,
        write_frame,
    };

    #[tokio::test]
    async fn notification_produces_no_response() {
        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "initialized"
        });

        let mut frame = serde_json::to_vec(&notification).expect("notification should serialize");
        frame.push(b'\n');

        let mut input = Cursor::new(frame);
        let (mut client_output, mut agent_output) = duplex(1024);

        run_acp(&mut input, &mut agent_output)
            .await
            .expect("notification should be processed through clean EOF");

        drop(agent_output);

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
        let mut reader = Cursor::new(b"");
        let mut writer = tokio::io::sink();

        run_acp(&mut reader, &mut writer)
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

        let request: Request<Value> =
            serde_json::from_value(request).expect("fixture should be a valid request shape");

        let error = validate_request(&request)
            .expect_err("JSON-RPC versions other than 2.0 should be rejected");

        assert_eq!(error.to_string(), "expected JSON-RPC version 2.0");
    }

    #[tokio::test]
    async fn frame_at_maximum_size_is_accepted() {
        let input = valid_request_frame_with_size(MAX_FRAME_BYTES);
        let mut reader = Cursor::new(input.as_bytes());
        let mut writer = tokio::io::sink();

        run_acp(&mut reader, &mut writer)
            .await
            .expect("a frame at the maximum size should be accepted");
    }

    #[tokio::test]
    async fn frame_over_maximum_size_is_rejected() {
        let input = valid_request_frame_with_size(MAX_FRAME_BYTES + 1);
        let mut reader = Cursor::new(input.as_bytes());
        let mut writer = tokio::io::sink();

        let error = run_acp(&mut reader, &mut writer)
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

        let mut cursor = Cursor::new(input.as_bytes());
        let mut writer = tokio::io::sink();
        let error = run_acp(&mut cursor, &mut writer)
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

        let mut cursor = Cursor::new(input.as_bytes());
        let mut writer = tokio::io::sink();
        run_acp(&mut cursor, &mut writer)
            .await
            .expect("to read both messages");
        assert_eq!(cursor.position() as usize, input.len());
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
        let mut reader = Cursor::new(&input);
        let mut writer = tokio::io::sink();

        run_acp(&mut reader, &mut writer)
            .await
            .expect("initialize request should succeed");
    }
}
