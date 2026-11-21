use std::collections::BTreeMap;
use std::env;

use crate::session::Session;
use crate::skills::Skill;
use crate::tools::{execute_tool_call, specs};
use crate::wire::ChatResponse;
use async_openai::Client;
use async_openai::config::OpenAIConfig;
use serde_json::{Value, json};
use tokio::sync::OnceCell;

#[allow(dead_code)]
pub(crate) trait AgentEventSink {
    async fn emit(&mut self, event: AgentEvent);
}

#[allow(dead_code)]
#[derive(Debug, PartialEq, Eq)]
pub enum ToolKind {
    Read,
    Edit,
    Execute,
}

pub(crate) struct NoOpAgentEventSink {}

impl AgentEventSink for NoOpAgentEventSink {
    async fn emit(&mut self, _event: AgentEvent) {}
}

#[allow(dead_code)]
#[derive(Debug, PartialEq, Eq)]
pub enum ReplyContent {
    TextContent { text: String },
}

#[allow(dead_code)]
#[derive(Debug, PartialEq, Eq)]
pub enum AgentEvent {
    AssistantText {
        message: String,
    },
    ToolRequested {
        tool_call_id: String,
        title: String,
        name: String,
        kind: ToolKind,
    },
    ToolStarted {
        tool_call_id: String,
    },
    ToolFinished {
        tool_call_id: String,
        content: ReplyContent,
        success: bool,
    },
}

#[derive(Debug, Default)]
pub struct Conversation {
    pub messages: Vec<Value>,
}

pub(crate) trait Model {
    fn complete(
        &self,
        request: Value,
    ) -> impl std::future::Future<Output = Result<Value, Box<dyn std::error::Error + Send + Sync>>> + Send;
}

impl Model for Client<OpenAIConfig> {
    async fn complete(
        &self,
        request: Value,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        Ok(self.chat().create_byot(request).await?)
    }
}

pub(crate) struct OpenRouterModel {
    client: OnceCell<Client<OpenAIConfig>>,
}

impl OpenRouterModel {
    pub(crate) fn new() -> Self {
        Self {
            client: OnceCell::new(),
        }
    }
}

impl Model for OpenRouterModel {
    async fn complete(
        &self,
        request: Value,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let client = self
            .client
            .get_or_try_init(|| async {
                let base_url = env::var("OPENROUTER_BASE_URL")
                    .unwrap_or_else(|_| "https://openrouter.ai/api/v1".to_string());

                let api_key = env::var("OPENROUTER_API_KEY").map_err(|_| {
                    std::io::Error::other(
                        "OPENROUTER_API_KEY is not set. Configure it in the process environment or a .env file, then restart the agent server."
                    )
                })?;

                let config = OpenAIConfig::new()
                    .with_api_base(base_url)
                    .with_api_key(api_key);

                Ok::<_, std::io::Error>(Client::with_config(config))
            })
            .await?;

        Ok(client.chat().create_byot(request).await?)
    }
}

fn expand_skill_prompt(prompt: &str, skills_by_name: &BTreeMap<String, Skill>) -> String {
    let Some(command) = prompt.split_whitespace().next() else {
        return prompt.to_string();
    };

    let Some(skill_name) = command.strip_prefix('/') else {
        return prompt.to_string();
    };

    match skills_by_name.get(skill_name) {
        Some(skill) => skill.content.clone(),
        None => prompt.to_string(),
    }
}

pub async fn turn<M, S>(
    client: &M,
    session: &mut Session,
    prompt: &str,
    sink: &mut S,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>>
where
    M: Model,
    S: AgentEventSink,
{
    let tools = specs();

    let expanded_prompt = expand_skill_prompt(prompt, &session.skills_by_name);
    session.conversation.messages.push(json!({
        "role": "user",
        "content": &expanded_prompt.to_string(),
    }));

    const MAX_LOOPS: u8 = 10;

    for _ in 0..MAX_LOOPS {
        let request = json!({
            "messages": session.conversation.messages,
            "model": "anthropic/claude-haiku-4.5",
            "tools": tools,
        });

        tracing::debug!(
          event = "llm_request",
          n_messages = session.conversation.messages.len(),
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

        session.conversation.messages.push(raw_message);

        let tool_calls = message.tool_calls.as_deref().unwrap_or_default();

        if tool_calls.is_empty() {
            return match &message.content {
                Some(content) => {
                    sink.emit(AgentEvent::AssistantText {
                        message: content.clone(),
                    })
                    .await;
                    Ok(content.clone())
                }
                None => Err(std::io::Error::other("Model completed without assistant text").into()),
            };
        }

        for tool_call in tool_calls {
            let result = execute_tool_call(tool_call).await;

            if let Some(error) = result["error"].as_str() {
                eprintln!("Tool call error: {}", error);
            }

            session.conversation.messages.push(json!({
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
    use std::{collections::VecDeque, vec};

    use crate::{
        scripted_model::{
            ScriptedModel,
            TurnResponse::{self, Done},
        },
        test_messages::assistant_reply,
    };

    struct TestEventSink {
        events: Vec<AgentEvent>,
    }

    impl TestEventSink {
        fn new() -> Self {
            Self { events: vec![] }
        }
    }

    impl AgentEventSink for TestEventSink {
        async fn emit(&mut self, event: AgentEvent) {
            self.events.push(event);
        }
    }

    use super::*;

    fn empty_session() -> Session {
        let cwd = std::env::current_dir().expect("test should have a current directory");
        Session::empty(cwd)
    }

    #[tokio::test]
    async fn completed_turn_returns_assistant_text() {
        let response = assistant_reply("scripted answer");

        let model = ScriptedModel::new(VecDeque::from([TurnResponse::Done {
            response: response.clone(),
        }]));

        let mut session = empty_session();
        let mut event_sink = TestEventSink::new();

        let _ = turn(&model, &mut session, "Hello, world!", &mut event_sink)
            .await
            .expect("turn should succeed");

        assert_eq!(event_sink.events.len(), 1);
        match &event_sink.events[0] {
            AgentEvent::AssistantText { message } => assert_eq!(message, "scripted answer"),
            _ => panic!("Should have received an AssistantText"),
        }
    }

    #[tokio::test]
    async fn second_turn_includes_first_turn_history() {
        let responses = [
            Done {
                response: assistant_reply("first answer"),
            },
            Done {
                response: assistant_reply("second answer"),
            },
        ];

        let model = ScriptedModel::new(VecDeque::from(responses));

        let mut session = empty_session();

        let mut event_sink = TestEventSink::new();

        turn(&model, &mut session, "first question", &mut event_sink)
            .await
            .expect("first turn should succeed");

        turn(&model, &mut session, "second question", &mut event_sink)
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
        let responses = [
            Done {
                response: assistant_reply("first answer"),
            },
            Done {
                response: assistant_reply("second answer"),
            },
        ];

        let model = ScriptedModel::new(VecDeque::from(responses));

        let mut first_session = empty_session();
        let mut first_event_sink = TestEventSink::new();

        let mut second_session = empty_session();
        let mut second_event_sink = TestEventSink::new();

        turn(
            &model,
            &mut first_session,
            "first question",
            &mut first_event_sink,
        )
        .await
        .expect("first conversation should succeed");

        turn(
            &model,
            &mut second_session,
            "second question",
            &mut second_event_sink,
        )
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
