use std::collections::BTreeMap;

use schemars::{JsonSchema, schema_for};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::tools::{ExecuteRetVal, Tool, ToolKind};

#[allow(unused)]
#[derive(Deserialize, JsonSchema)]
pub(crate) struct BashArgs {
    #[schemars(description = "The command to execute")]
    pub command: String,
}

#[allow(unused)]
pub(crate) struct BashTool {
    pub name: String,
    pub kind: ToolKind,
    pub description: String,
    pub params: Value,
}

impl Tool for BashTool {
    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn kind(&self) -> ToolKind {
        self.kind
    }

    fn description(&self) -> &str {
        "Execute a shell command"
    }

    fn params(&self) -> Value {
        // TODO Do not use unwrap here
        serde_json::to_value(schema_for!(BashArgs)).unwrap()
    }

    fn execute(&self, args_by_name: BTreeMap<String, String>) -> ExecuteRetVal<'_> {
        Box::pin(async move {
            if let Some(command) = args_by_name.get("command") {
                tracing::debug!(command = %command, "Deserialized Bash arguments");
                let output = match tokio::process::Command::new("bash")
                    .arg("-c")
                    .arg(command)
                    .kill_on_drop(true)
                    .output()
                    .await
                {
                    Ok(output) => output,
                    Err(err) => {
                        return Ok(json!({
                            "ok": false,
                            "error": format!("Could run the command successfully. {}", err)
                        }));
                    }
                };

                Ok(json!({
                    "ok": true,
                    "stdout": &output.stdout,
                    "stderr": &output.stderr
                }))
            } else {
                println!("{:?}", args_by_name);
                todo!()
            }
        })
    }

    fn build_title(&self, _args_by_name: &BTreeMap<String, String>) -> String {
        "Run a bash command".to_string()
    }
}
