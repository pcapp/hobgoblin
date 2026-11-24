use std::collections::HashMap;

use crate::wire::ToolCall;
use schemars::{JsonSchema, schema_for};
use serde::Deserialize;
use serde_json::{Value, json};

const READ: &str = "Read";
const WRITE: &str = "Write";
const BASH: &str = "Bash";

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub(crate) enum ToolKind {
    Read,
    Edit,
    Execute,
}

pub(crate) struct Tool {
    pub name: String,
    pub kind: ToolKind,
    pub description: String,
    pub params: Value,
}

impl Tool {
    pub fn spec(&self) -> Value {
        json!({"type": "function",
        "function": {
          "name": self.name,
          "description": self.description,
          "parameters": self.params
        }})
    }
}

pub fn get_tools() -> Result<HashMap<String, Tool>, serde_json::Error> {
    let read_tool = Tool {
        name: READ.to_string(),
        kind: ToolKind::Read,
        description: "Read and return the contents of a file".to_string(),
        params: serde_json::to_value(schema_for!(ReadArgs))?,
    };

    let write_tool = Tool {
        name: WRITE.to_string(),
        kind: ToolKind::Edit,
        description: "Write content to a file".to_string(),
        params: serde_json::to_value(schema_for!(WriteArgs))?,
    };

    let bash_tool = Tool {
        name: BASH.to_string(),
        kind: ToolKind::Execute,
        description: "Execute a shell command".to_string(),
        params: serde_json::to_value(schema_for!(BashArgs))?,
    };

    Ok(HashMap::from([
        (READ.to_string(), read_tool),
        (WRITE.to_string(), write_tool),
        (BASH.to_string(), bash_tool),
    ]))
}

#[derive(Deserialize, JsonSchema)]
struct ReadArgs {
    file_path: String,
}

#[derive(Deserialize, JsonSchema)]
struct WriteArgs {
    file_path: String,
    content: String,
}

#[derive(Deserialize, JsonSchema)]
struct BashArgs {
    #[schemars(description = "The command to execute")]
    command: String,
}

fn read_file_tool(arguments: &str) -> Value {
    let args: ReadArgs = match serde_json::from_str(arguments) {
        Ok(args) => args,
        Err(err) => {
            return json!({
                "ok": false,
                "error": format!("Invalid Read arguments: {}", err)
            });
        }
    };

    match std::fs::read_to_string(args.file_path) {
        Ok(contents) => json!({
          "ok": true,
          "content": contents
        }),
        Err(err) => json!({
          "ok": false,
          "error": err.to_string()
        }),
    }
}

fn write_file_tool(arguments: &str) -> Value {
    let args: WriteArgs = match serde_json::from_str(arguments) {
        Ok(args) => args,
        Err(err) => {
            return json!({
                "ok": false,
                "error": format!("Invalid Write arguments: {}", err)
            });
        }
    };

    match std::fs::write(args.file_path, args.content) {
        Ok(contents) => json!({
          "ok": true,
          "content": contents
        }),
        Err(err) => json!({
          "ok": false,
          "error": err.to_string()
        }),
    }
}

async fn run_bash_tool(arguments: &str) -> Value {
    let args: BashArgs = match serde_json::from_str(arguments) {
        Ok(args) => args,
        Err(err) => {
            return json!({
                "ok": false,
                "error": format!("Invalid Write arguments: {}", err)
            });
        }
    };

    tracing::debug!(command = %args.command, "Deserialized Bash arguments");
    let output = match tokio::process::Command::new("bash")
        .arg("-c")
        .arg(&args.command)
        .kill_on_drop(true)
        .output()
        .await
    {
        Ok(output) => output,
        Err(err) => {
            return json!({
                "ok": false,
                "error": format!("Could run the command successfully. {}", err)
            });
        }
    };

    json!({
        "ok": true,
        "stdout": &output.stdout,
        "stderr": &output.stderr
    })
}

pub async fn execute_tool_call(call: &ToolCall) -> Value {
    match call.function.name.as_str() {
        READ => read_file_tool(&call.function.arguments),
        WRITE => write_file_tool(&call.function.arguments),
        BASH => run_bash_tool(&call.function.arguments).await,

        unimplemented_tool => json!({
          "ok": false,
          "error": format!("Unimplemented tool: {}", unimplemented_tool)
        }),
    }
}

#[cfg(test)]
mod tests {
    use schemars::schema_for;
    use serde_json::json;

    use crate::tools::{READ, ReadArgs, Tool};

    #[test]
    fn tool_generates_spec() {
        let read_tool = Tool {
            name: READ.to_string(),
            kind: crate::tools::ToolKind::Read,
            description: "Read and return the contents of a file".to_string(),
            params: serde_json::to_value(schema_for!(ReadArgs)).expect("to serialize"),
        };

        let expected = json!({"type": "function",
        "function": {
          "name": READ,
          "description": "Read and return the contents of a file",
          "parameters": schema_for!(ReadArgs)
        }});

        assert_eq!(read_tool.spec(), expected);
    }
}
