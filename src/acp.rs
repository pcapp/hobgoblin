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
    let n = reader.read_line(&mut input).await?;

    if n == 0 {
        return Ok(());
    }

    let request: Request<Value> = serde_json::from_str(&input)?;
    validate_request(&request)?;

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
