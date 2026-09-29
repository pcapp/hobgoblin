use crate::wire::ToolCall;
use schemars::{JsonSchema, schema_for};
use serde::Deserialize;
use serde_json::{Value, json};

const READ: &str = "Read";
const WRITE: &str = "Write";
const BASH: &str = "Bash";

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

fn read_spec() -> Value {
    json!({"type": "function",
    "function": {
      "name": READ,
      "description": "Read and return the contents of a file",
      "parameters": schema_for!(ReadArgs)
    }})
}

fn write_spec() -> Value {
    json!({
      "type": "function",
      "function": {
        "name": WRITE,
        "description": "Write content to a file",
        "parameters": schema_for!(WriteArgs)
      }
    })
}

fn bash_spec() -> Value {
    json!({
      "type": "function",
      "function": {
        "name": BASH,
        "description": "Execute a shell command",
        "parameters": schema_for!(BashArgs)
      }
    })
}

pub fn specs() -> Value {
    json!([read_spec(), write_spec(), bash_spec()])
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

fn run_bash_tool(arguments: &str) -> Value {
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
    let output = match std::process::Command::new("bash")
        .arg("-c")
        .arg(&args.command)
        .output()
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

pub fn execute_tool_call(call: &ToolCall) -> Value {
    match call.function.name.as_str() {
        READ => read_file_tool(&call.function.arguments),
        WRITE => write_file_tool(&call.function.arguments),
        BASH => run_bash_tool(&call.function.arguments),

        unimplemented_tool => json!({
          "ok": false,
          "error": format!("Unimplemented tool: {}", unimplemented_tool)
        }),
    }
}
