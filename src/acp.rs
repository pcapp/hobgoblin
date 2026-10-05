use serde::Deserialize;
use serde_json::Value;

use tokio::io::{AsyncBufRead, AsyncBufReadExt};

#[derive(Deserialize)]
struct Request<T> {
    jsonrpc: String,
    id: Value,

    #[allow(dead_code)]
    method: String,

    #[allow(dead_code)]
    params: Option<T>,
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

pub async fn run_acp<R: AsyncBufRead + Unpin>(
    reader: &mut R,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut input = String::new();

    loop {
        input.clear();
        let n = reader.read_line(&mut input).await?;
        if n == 0 {
            break;
        } else if !input.ends_with('\n') {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "the last message should have a newline",
            )
            .into());
        }

        let request: Request<Value> = serde_json::from_str(&input)?;
        validate_request(&request)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use serde_json::Value;

    use crate::acp::{Request, run_acp, validate_request};

    #[tokio::test]
    async fn clean_eof_exits_successfully() {
        let mut reader = Cursor::new(b"");

        run_acp(&mut reader)
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
        let error = run_acp(&mut cursor)
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
        run_acp(&mut cursor).await.expect("to read both messages");
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

        run_acp(&mut reader)
            .await
            .expect("initialize request should succeed");
    }
}
