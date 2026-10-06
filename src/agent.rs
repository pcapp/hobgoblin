use crate::skills::FileSystemSkillLoader;
use crate::tools::{execute_tool_call, specs};
use crate::wire::ChatResponse;
use async_openai::Client;
use async_openai::config::OpenAIConfig;
use serde_json::{Value, json};

#[derive(Debug, Default)]
pub struct Conversation {
    pub messages: Vec<Value>,
}

pub(crate) trait Model {
    async fn complete(&self, request: Value) -> Result<Value, Box<dyn std::error::Error>>;
}

impl Model for Client<OpenAIConfig> {
    async fn complete(&self, request: Value) -> Result<Value, Box<dyn std::error::Error>> {
        Ok(self.chat().create_byot(request).await?)
    }
}

pub async fn turn<M: Model>(
    client: &M,
    conversation: &mut Conversation,
    prompt: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let tools = specs();

    conversation.messages.push(json!({
        "role": "user",
        "content": &prompt.to_string(),
    }));

    const MAX_LOOPS: u8 = 10;

    for _ in 0..MAX_LOOPS {
        let request = json!({
            "messages": conversation.messages,
            "model": "anthropic/claude-haiku-4.5",
            "tools": tools,
        });

        tracing::debug!(
          event = "llm_request",
          n_messages = conversation.messages.len(),
          payload = %request,
        );

        let response: Value = client.complete(request).await?;
        tracing::debug!(
          event = "llm_response",
          payload = %response,
        );

        let raw_message = response["choices"][0]["message"].clone();

        let response = serde_json::from_value::<ChatResponse>(response)?;

        let Some(choice) = response.choices.first() else {
            return Err(std::io::Error::other("Model returned no choices.").into());
        };

        let message = &choice.message;

        conversation.messages.push(raw_message);

        let tool_calls = message.tool_calls.as_deref().unwrap_or_default();

        if tool_calls.is_empty() {
            return match &message.content {
                Some(content) => Ok(content.clone()),
                None => Err(std::io::Error::other("Model completed without assistant text").into()),
            };
        }

        for tool_call in tool_calls {
            let result = execute_tool_call(tool_call);

            if let Some(error) = result["error"].as_str() {
                eprintln!("Tool call error: {}", error);
            }

            conversation.messages.push(json!({
              "role": "tool",
              "tool_call_id": tool_call.id,
              "content": result.to_string()
            }));
        }
    }

    Err(std::io::Error::other(format!(
        "The agentic loop exceeded the max iterations ({}).",
        MAX_LOOPS,
    ))
    .into())
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;
    use std::sync::Mutex;

    struct ScriptedModel {
        responses: Mutex<VecDeque<Value>>,
        requests: Mutex<Vec<Value>>,
    }

    impl ScriptedModel {
        fn new(responses: impl IntoIterator<Item = Value>) -> Self {
            Self {
                responses: Mutex::new(responses.into_iter().collect()),
                requests: Mutex::new(Vec::new()),
            }
        }
    }

    impl Model for ScriptedModel {
        async fn complete(&self, request: Value) -> Result<Value, Box<dyn std::error::Error>> {
            self.requests.lock().unwrap().push(request);

            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| std::io::Error::other("Scripted model ran out of responses").into())
        }
    }

    #[tokio::test]
    async fn completed_turn_returns_assistant_text() {
        let model = ScriptedModel::new([json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "scripted answer",
                    "tool_calls": null
                }
            }]
        })]);

        let mut conversation = Conversation::default();

        let answer = turn(&model, &mut conversation, "Hello, world!")
            .await
            .expect("turn should succeed");

        assert_eq!(answer, "scripted answer");
    }

    #[tokio::test]
    async fn second_turn_includes_first_turn_history() {
        let model = ScriptedModel::new([
            json!({
                "choices": [{
                    "message": {
                        "role": "assistant",
                        "content": "first answer",
                        "tool_calls": null
                    }
                }]
            }),
            json!({
                "choices": [{
                    "message": {
                        "role": "assistant",
                        "content": "second answer",
                        "tool_calls": null
                    }
                }]
            }),
        ]);

        let mut conversation = Conversation::default();

        turn(&model, &mut conversation, "first question")
            .await
            .expect("first turn should succeed");

        turn(&model, &mut conversation, "second question")
            .await
            .expect("second turn should succeed");

        let requests = model.requests.lock().unwrap();

        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[1]["messages"],
            json!([
                {
                    "role": "user",
                    "content": "first question"
                },
                {
                    "role": "assistant",
                    "content": "first answer",
                    "tool_calls": null
                },
                {
                    "role": "user",
                    "content": "second question"
                }
            ])
        );
    }

    #[tokio::test]
    async fn separate_conversations_do_not_share_messages() {
        let model = ScriptedModel::new([
            json!({
                "choices": [{
                    "message": {
                        "role": "assistant",
                        "content": "first answer",
                        "tool_calls": null
                    }
                }]
            }),
            json!({
                "choices": [{
                    "message": {
                        "role": "assistant",
                        "content": "second answer",
                        "tool_calls": null
                    }
                }]
            }),
        ]);

        let mut first_conversation = Conversation::default();
        let mut second_conversation = Conversation::default();

        turn(&model, &mut first_conversation, "first question")
            .await
            .expect("first conversation should succeed");

        turn(&model, &mut second_conversation, "second question")
            .await
            .expect("second conversation should succeed");

        let requests = model.requests.lock().unwrap();

        assert_eq!(
            requests[0]["messages"],
            json!([{
                "role": "user",
                "content": "first question"
            }])
        );

        assert_eq!(
            requests[1]["messages"],
            json!([{
                "role": "user",
                "content": "second question"
            }])
        );
    }
}
