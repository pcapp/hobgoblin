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

pub fn make_prompt_request(id: u64, session_id: &str, prompt: &str) -> Value {
    serde_json::json!({
      "jsonrpc": "2.0",
      "id": id,
      "method": "session/prompt",
      "params": {
        "sessionId": session_id,
        "prompt": [
          {
            "type": "text",
            "text": prompt
          }
        ]
      }
    })
}
