use std::collections::BTreeMap;
use std::env;
use std::error::Error;
use std::fmt::Display;

use crate::agent::EventSinkError::DeliveryFailed;
use crate::session::Session;
use crate::skills::Skill;
use crate::tools::{ToolKind, execute_tool_call, get_tools};
use crate::wire::ChatResponse;
use async_openai::Client;
use async_openai::config::OpenAIConfig;
use serde_json::{Value, json};
use tokio::sync::OnceCell;

#[allow(dead_code)]
#[derive(Debug)]
pub(crate) enum EventSinkError {
    DeliveryFailed,
}

impl Display for EventSinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeliveryFailed => write!(f, "Event delivery failed"),
        }
    }
}

impl Error for EventSinkError {}

#[allow(dead_code)]
pub(crate) trait AgentEventSink {
    async fn emit(&mut self, event: AgentEvent) -> Result<(), EventSinkError>;
}

pub(crate) struct NoOpAgentEventSink {}

impl AgentEventSink for NoOpAgentEventSink {
    async fn emit(&mut self, _event: AgentEvent) -> Result<(), EventSinkError> {
        Ok(())
    }
}

#[allow(dead_code)]
#[derive(Debug, PartialEq, Eq)]
pub enum ReplyContent {
    TextContent { text: String },
}

#[allow(dead_code)]
#[derive(Debug, PartialEq, Eq)]
pub enum ToolCallStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
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
        arguments: Vec<String>,
    },
    ToolUpdate {
        tool_call_id: String,
        status: ToolCallStatus,
        content: Vec<ReplyContent>,
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
    let tools: Vec<Value> = get_tools().iter().map(|t| t.spec()).collect();
    let tools = serde_json::to_value(tools)?;

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
                    let () = sink
                        .emit(AgentEvent::AssistantText {
                            message: content.clone(),
                        })
                        .await?;
                    Ok(content.clone())
                }
                None => Err(std::io::Error::other("Model completed without assistant text").into()),
            };
        } else if let Some(message) = &message.content {
            let () = sink
                .emit(AgentEvent::AssistantText {
                    message: message.clone(),
                })
                .await?;
        }

        for tool_call in tool_calls {
            // let tool_request = ToolRequested {
            //     tool_call_id: tool_call.id.clone(),
            //     title: todo!(),
            //     name: todo!(),
            //     kind: todo!(),
            //     arguments: todo!(),
            // };
            // let () = sink.emit(tool_request).await?;
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
        agent::AgentEvent::*,
        scripted_model::{
            ScriptedModel,
            TurnResponse::{self, Done},
        },
        test_messages::{assistant_reply, assistant_reply_with_tool_calls, make_tool_call},
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
        async fn emit(&mut self, event: AgentEvent) -> Result<(), EventSinkError> {
            self.events.push(event);
            Ok(())
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
            AssistantText { message } => assert_eq!(message, "scripted answer"),
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

    fn check_assistant_text(event: &AgentEvent, expected: &str) {
        match event {
            AssistantText { message } => assert_eq!(message, expected),
            _ => panic!("Should have received an AssistantText"),
        }
    }

    #[tokio::test]
    async fn tool_requests_with_text_show_agent_text() {
        let tool_calls = vec![make_tool_call("tool_request", "test", "")];
        let responses = [
            Done {
                response: assistant_reply_with_tool_calls("I'll try the test tool.", tool_calls),
            },
            Done {
                response: assistant_reply("The answer is: banana"),
            },
        ];

        let model = ScriptedModel::new(VecDeque::from(responses));

        let mut session = empty_session();
        let mut event_sink = TestEventSink::new();

        turn(&model, &mut session, "first question", &mut event_sink)
            .await
            .expect("first conversation should succeed");

        assert_eq!(event_sink.events.len(), 2);

        check_assistant_text(&event_sink.events[0], "I'll try the test tool.");
        check_assistant_text(&event_sink.events[1], "The answer is: banana");
    }

    //     #[tokio::test]
    //     async fn tool_emits_lifecycle_events_for_successful_usage() {
    //         let expected_tool_id = "tool_request";
    //         let expected_tool_name = "Read";
    //         let expected_tool_title = "test";
    //         let tool_calls = vec![make_tool_call(expected_tool_id, expected_tool_name, "")];
    //         let responses = [
    //             Done {
    //                 response: assistant_reply_with_tool_calls("I'll try the read tool.", tool_calls),
    //             },
    //             Done {
    //                 response: assistant_reply("The answer is: banana"),
    //             },
    //         ];
    //
    //         let model = ScriptedModel::new(VecDeque::from(responses));
    //
    //         let mut session = empty_session();
    //         let mut event_sink = TestEventSink::new();
    //
    //         turn(&model, &mut session, "first question", &mut event_sink)
    //             .await
    //             .expect("first conversation should succeed");
    //
    //         assert_eq!(event_sink.events.len(), 2);
    //
    //         check_assistant_text(&event_sink.events[0], "I'll try the read tool.");
    //
    //         match &event_sink.events[1] {
    //             ToolRequested {
    //                 tool_call_id,
    //                 title,
    //                 name,
    //                 kind,
    //                 arguments,
    //             } => {
    //                 assert_eq!(tool_call_id, expected_tool_id);
    //                 assert_eq!(title, expected_tool_title);
    //                 assert_eq!(name, expected_tool_name);
    //                 assert!(matches!(kind, ToolKind::Read));
    //                 assert_eq!(arguments.len(), 0);
    //             }
    //             _ => panic!("Expected a ToolRequested event."),
    //         }
    //
    //         match &event_sink.events[2] {
    //             ToolUpdate {
    //                 tool_call_id,
    //                 status,
    //                 content,
    //             } => {
    //                 assert_eq!(tool_call_id, expected_tool_id);
    //                 assert!(matches!(status, ToolCallStatus::Completed));
    //                 let expected_content = TextContent {
    //                     text: String::from("Test tool result"),
    //                 };
    //                 assert_eq!(content.len(), 1);
    //                 assert_eq!(expected_content, content[0]);
    //             }
    //             _ => panic!("Expected a ToolUpdated event."),
    //         }
    //
    //         check_assistant_text(&event_sink.events[0], "The answer is: banana");
    //     }
}
