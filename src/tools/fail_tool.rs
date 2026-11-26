use crate::tools::{ExecuteRetVal, Tool, ToolKind};
use schemars::{JsonSchema, schema_for};
use serde_json::Value;
use std::collections::BTreeMap;
use std::io;

pub(crate) struct FailTool {
    pub name: String,
    pub kind: ToolKind,
    #[allow(unused)]
    pub description: String,
    pub title: String,
    #[allow(unused)]
    pub params: Value,
}

#[derive(JsonSchema)]
pub(crate) struct FailArgs {}

impl Tool for FailTool {
    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn kind(&self) -> ToolKind {
        self.kind
    }

    fn description(&self) -> &str {
        "Test tool failure"
    }

    fn build_title(&self, _args_by_name: &BTreeMap<String, String>) -> String {
        self.title.clone()
    }

    fn params(&self) -> Value {
        serde_json::to_value(schema_for!(FailArgs)).expect("to parse")
    }

    fn execute(&self, _args_by_name: BTreeMap<String, String>) -> ExecuteRetVal<'_> {
        Box::pin(async move { Err(io::Error::other("Test tool error")) })
    }
}
