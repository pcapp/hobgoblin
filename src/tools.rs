use std::{
    collections::{BTreeMap, HashMap},
    io,
    pin::Pin,
    sync::LazyLock,
};

mod bash_tool;
mod read_tool;
mod write_tool;

use crate::{
    tools::{
        bash_tool::{BashArgs, BashTool},
        read_tool::{ReadArgs, ReadTool},
        write_tool::{WriteArgs, WriteTool},
    },
    wire::ToolCall,
};
use regex::Regex;
use schemars::schema_for;
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

pub(crate) type ExecuteRetVal<'a> =
    Pin<Box<dyn Future<Output = Result<Value, io::Error>> + Send + 'a>>;

pub(crate) trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn kind(&self) -> ToolKind;
    fn description(&self) -> &str;
    fn build_title(&self, args_by_name: &BTreeMap<String, String>) -> String;
    fn params(&self) -> Value;
    fn execute(&self, args_by_name: BTreeMap<String, String>) -> ExecuteRetVal<'_>;

    fn spec(&self) -> Value {
        json!({"type": "function",
        "function": {
          "name": self.name(),
          "description": self.description(),
          "parameters": self.params()
        }})
    }
}

pub fn get_tools() -> Result<HashMap<String, Box<dyn Tool>>, serde_json::Error> {
    let read_tool: Box<dyn Tool> = Box::new(ReadTool {
        name: READ.to_string(),
        kind: ToolKind::Read,
        description: "Read and return the contents of a file".to_string(),
        params: serde_json::to_value(schema_for!(ReadArgs))?,
    });

    let write_tool: Box<dyn Tool> = Box::new(WriteTool {
        name: WRITE.to_string(),
        kind: ToolKind::Edit,
        description: "Write content to a file".to_string(),
        params: serde_json::to_value(schema_for!(WriteArgs))?,
    });

    let bash_tool: Box<dyn Tool> = Box::new(BashTool {
        name: BASH.to_string(),
        kind: ToolKind::Execute,
        description: "Execute a shell command".to_string(),
        params: serde_json::to_value(schema_for!(BashArgs))?,
    });

    Ok(HashMap::<String, Box<dyn Tool>>::from([
        (READ.to_string(), read_tool),
        (WRITE.to_string(), write_tool),
        (BASH.to_string(), bash_tool),
    ]))
}

pub async fn execute_tool_call(call: &ToolCall, tools: &HashMap<String, Box<dyn Tool>>) -> Value {
    let tool_name = call.function.name.as_str();

    if let Some(tool) = tools.get(tool_name) {
        let args_by_name = parse_arguments(&call.function.arguments).unwrap();
        tool.execute(args_by_name).await.unwrap()
    } else {
        json!({
              "ok": false,
              "error": format!("Unimplemented tool: {}", tool_name)
        })
    }
}

static KEY_VALUE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#""(?<key>\w+)"\s*:\s*"(?<value>[^"]*)""#).unwrap());

pub fn parse_arguments(args: &str) -> Result<BTreeMap<String, String>, regex::Error> {
    let mut arg_by_name = BTreeMap::new();

    for group in KEY_VALUE_RE.captures_iter(args) {
        arg_by_name.insert(group["key"].to_string(), group["value"].to_string());
    }

    Ok(arg_by_name)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use schemars::schema_for;
    use serde_json::json;

    use crate::tools::{READ, ReadTool, Tool, parse_arguments, read_tool::ReadArgs};

    #[test]
    fn should_parse_tool_call_args() {
        let args = "{\"file_path\": \"src/tools.rs\", \"content\": \"content\"}";
        let arg_by_name = parse_arguments(args).expect("to parse successfully");

        let expected = BTreeMap::from([
            ("file_path".to_string(), "src/tools.rs".to_string()),
            ("content".to_string(), "content".to_string()),
        ]);
        assert_eq!(expected, arg_by_name)
    }

    #[test]
    fn tool_generates_spec() {
        let read_tool = ReadTool {
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
