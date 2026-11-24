use std::collections::BTreeMap;

use schemars::{JsonSchema, schema_for};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::tools::{ExecuteRetVal, Tool, ToolKind};

#[allow(unused)]
#[derive(Deserialize, JsonSchema)]
pub(crate) struct WriteArgs {
    file_path: String,
    content: String,
}

#[allow(unused)]
pub(crate) struct WriteTool {
    pub name: String,
    pub kind: ToolKind,
    pub description: String,
    pub params: Value,
}

impl Tool for WriteTool {
    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn kind(&self) -> ToolKind {
        self.kind
    }

    fn description(&self) -> &str {
        "Read and return the contents of a file"
    }

    fn params(&self) -> Value {
        // TODO Do not use unwrap here
        serde_json::to_value(schema_for!(WriteArgs)).unwrap()
    }

    fn execute(&self, args_by_name: BTreeMap<String, String>) -> ExecuteRetVal<'_> {
        Box::pin(async move {
            if let Some(file_path) = args_by_name.get("file_path")
                && let Some(content) = args_by_name.get("content")
            {
                match std::fs::write(file_path, content) {
                    Ok(content) => Ok(json!({
                      "ok": true,
                      "content": content
                    })),
                    Err(err) => Ok(json!({
                      "ok": false,
                      "error": err.to_string()
                    })),
                }
            } else {
                todo!()
            }
        })
    }

    fn build_title(&self, args_by_name: &BTreeMap<String, String>) -> String {
        if let Some(file_path) = args_by_name.get("file_path") {
            format!("Write to {file_path}")
        } else {
            "Write".to_string()
        }
    }
}
