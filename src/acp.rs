use serde::Deserialize;
use serde_json::Value;

use crate::agent;
use std::io::{BufRead, Write};

#[derive(Debug, Deserialize)]
struct ClientInfo {
    name: String,
    title: String,
    version: String,
}

#[derive(Debug, Default, Deserialize)]
struct AuthCapabilities {
    #[serde(default)]
    terminal: bool,
}

#[derive(Debug, Deserialize)]
enum BooleanConfigOptionCapabilities {
    NotSupported,
    Supported,
}

#[derive(Debug, Deserialize)]
struct SessionConfigOptionsCapabilities {
    boolean: Option<BooleanConfigOptionCapabilities>,
}

#[derive(Debug, Deserialize)]
struct ClientSessionCapabilities {
    config_options: Option<SessionConfigOptionsCapabilities>,
}

#[derive(Debug, Deserialize)]
struct ElicitiationCapabilities {}

#[derive(Debug, Default, Deserialize)]
struct FileSystemCapabilities {
    #[serde(default)]
    readTextFile: bool,
    #[serde(default)]
    writeTextFile: bool,
}

#[derive(Debug, Deserialize)]
struct ClientCapabilities {
    #[serde(default)]
    auth: AuthCapabilities,

    elicitation: Option<ElicitiationCapabilities>,

    #[serde(default)]
    fs: FileSystemCapabilities,

    session: Option<ClientSessionCapabilities>,

    #[serde(default)]
    terminal: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InitializeRequest {
    protocol_version: u8,
    client_capabilities: ClientCapabilities,
    client_info: ClientInfo,
}

#[derive(Debug, Deserialize)]
struct Request<T> {
    jsonrpc: String,
    id: Value,
    method: String,
    params: T,
}

pub async fn run_acp<M: agent::Model, R: BufRead, W: Write>(
    _client: &M,
    reader: &mut R,
    writer: &mut W,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut input = String::new();
    reader.read_line(&mut input)?;

    let request: Request<InitializeRequest> = serde_json::from_str(&input)?;

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
