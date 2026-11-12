use serde_json::Value;

pub fn assistant_reply(text: &str) -> Value {
    serde_json::json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "content": text,
                "tool_calls": null
            }
        }]
    })
}
