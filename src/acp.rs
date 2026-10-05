use crate::agent;
use std::io::{BufRead, Write};

pub async fn run_acp<M: agent::Model, R: BufRead, W: Write>(
    _client: &M,
    reader: &mut R,
    writer: &mut W,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut buf = String::new();
    reader.read_line(&mut buf)?;

    println!("{buf}");

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use crate::{acp::run_acp, agent};
    struct ScriptedModel {}

    impl agent::Model for ScriptedModel {
        async fn complete(
            &self,
            _request: serde_json::Value,
        ) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
            Ok(serde_json::json!({}))
        }
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

        let client = ScriptedModel {};
        let mut input =
            serde_json::to_string(&init_request).expect("initialize message should serialize");
        input.push('\n');
        let mut reader = Cursor::new(&input);
        let mut writer = Vec::new();

        run_acp(&client, &mut reader, &mut writer)
            .await
            .expect("initialize request should succeed");
    }
}
