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

pub fn make_initialize() -> Value {
    serde_json::json!({
      "jsonrpc": "2.0",
      "id": 1,
      "method": "initialize",
      "params": {
          "protocolVersion": 1,
          "clientCapabilities": {},
          "clientInfo": {
              "version": "1.0.0"
          }
      }
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

pub fn make_agent_message_chunk(session_id: &str, text: &str) -> Value {
    serde_json::json!({
        "jsonrpc":"2.0",
        "method":"session/update",
        "params":{
            "sessionId":session_id,
            "update":{
                "content":{
                    "text": text,
                    "type": "text"},
                "sessionUpdate":"agent_message_chunk"
            }
        }
    })
}

pub fn make_cancel_notification(session_id: &str) -> Value {
    serde_json::json!({
      "jsonrpc": "2.0",
      "method": "session/cancel",
      "params": {
        "sessionId": session_id
      }
    })
}
