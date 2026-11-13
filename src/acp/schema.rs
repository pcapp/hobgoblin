use std::path::PathBuf;

use serde::Deserialize;
use serde_json::Value;

use crate::acp::session_registry::SessionId;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelParams {
    pub session_id: SessionId,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct InitializeParams {
    pub protocol_version: u16,

    pub client_info: Option<Implementation>,

    #[serde(default)]
    pub client_capabilities: ClientCapabilities,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct Implementation {
    pub name: String,
    pub title: Option<String>,
    pub version: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
#[allow(dead_code)]
pub struct ClientCapabilities {
    pub fs: FileSystemCapabilities,
    pub auth: AuthCapabilities,
    pub terminal: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
#[allow(dead_code)]
pub struct FileSystemCapabilities {
    pub read_text_file: bool,
    pub write_text_file: bool,
}

#[derive(Debug, Default, Deserialize)]
#[allow(dead_code)]
#[serde(default)]
pub struct AuthCapabilities {
    pub terminal: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewSessionParams {
    pub cwd: PathBuf,

    #[allow(
        dead_code,
        reason = "ACP requires mcpServers, but MCP support is intentionally deferred"
    )]
    pub mcp_servers: Vec<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptParams {
    pub session_id: SessionId,
    pub prompt: Vec<PromptContentBlock>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PromptContentBlock {
    Text {
        text: String,
    },
    #[serde(other)]
    Unsupported,
}
