use std::collections::BTreeMap;

use schemars::{JsonSchema, schema_for};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::tools::{ExecuteRetVal, Tool, ToolKind};

#[derive(Deserialize, JsonSchema)]
pub(crate) struct ReadArgs {
    #[allow(unused)]
    file_path: String,
}

pub(crate) struct ReadTool {
    pub name: String,
    pub kind: ToolKind,
    #[allow(unused)]
    pub description: String,
    #[allow(unused)]
    pub params: Value,
}

impl Tool for ReadTool {
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
        serde_json::to_value(schema_for!(ReadArgs)).unwrap()
    }

    fn execute(&self, args_by_name: BTreeMap<String, String>) -> ExecuteRetVal<'_> {
        Box::pin(async move {
            if let Some(file_path) = args_by_name.get("file_path") {
                match std::fs::read_to_string(file_path) {
                    Ok(contents) => Ok(json!({
                      "ok": true,
                      "content": contents
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
}
