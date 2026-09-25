use crate::tools::{execute_tool_call, specs};
use crate::wire::ChatResponse;
use async_openai::Client;
use async_openai::config::OpenAIConfig;
use serde_json::{Value, json};

pub async fn run(
    client: &Client<OpenAIConfig>,
    prompt: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let tools = specs();
    let mut messages: Vec<Value> = vec![json!({
        "role": "user",
        "content": &prompt.to_string(),
    })];
    const MAX_LOOPS: u8 = 10;

    for _ in 0..MAX_LOOPS {
        let request = json!({
            "messages": messages,
            "model": "anthropic/claude-haiku-4.5",
            "tools": tools,
        });

        tracing::debug!(
          event = "llm_request",
          n_messages = messages.len(),
          payload = %request,
        );

        let response: Value = client.chat().create_byot(request).await?;
        tracing::debug!(
          event = "llm_response",
          payload = %response,
        );

        let raw_message = response["choices"][0]["message"].clone();

        let response = match serde_json::from_value::<ChatResponse>(response) {
            Ok(parsed_response) => parsed_response,
            Err(err) => {
                eprintln!("Received a malformed OpenRouter response.");
                eprintln!("Error: {}", err);
                return Ok(());
            }
        };

        let Some(choice) = response.choices.first() else {
            eprintln!("No choices returned!");
            return Ok(());
        };

        let message = &choice.message;

        messages.push(raw_message);

        let tool_calls = message.tool_calls.as_deref().unwrap_or_default();

        if tool_calls.is_empty() {
            if let Some(content) = &message.content {
                println!("{}", content);
            }
            return Ok(());
        }

        for tool_call in tool_calls {
            let result = execute_tool_call(tool_call);

            if let Some(error) = result["error"].as_str() {
                eprintln!("Tool call error: {}", error);
            }

            messages.push(json!({
              "role": "tool",
              "tool_call_id": tool_call.id,
              "content": result.to_string()
            }));
        }
    }

    eprintln!(
        "The agentic loop exceeded the max iterations ({}).",
        MAX_LOOPS,
    );

    Ok(())
}
