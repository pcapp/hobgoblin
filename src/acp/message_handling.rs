use std::sync::Arc;

use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

use crate::{
    acp::{
        AcpError, ConnectionState, Request,
        json_rpc::{ErrorResponse, SuccessResponse},
        schema::{InitializeParams, NewSessionParams, PromptContentBlock, PromptParams},
        session_registry::{SessionEntry, SessionId, get_session},
    },
    agent::{self, Model},
    session::Session,
};

pub async fn dispatch_request<M: Model + Send + Sync + 'static>(
    request: Request<Value>,
    state: &mut ConnectionState,
    model: &Arc<M>,
    sender: &mpsc::Sender<Value>,
) -> Result<Option<Value>, AcpError> {
    if request.method == "initialize" {
        let result = match handle_initialize(request, state).await {
            Ok(response) => serde_json::to_value(response)?,
            Err(err) => serde_json::to_value(err)?,
        };

        return Ok(Some(result));
    }

    if !state.initialized {
        let response = ErrorResponse::connection_not_initialized(request.id);

        let response = serde_json::to_value(response)?;
        return Ok(Some(response));
    }

    if request.method == "session/new" {
        let result = match handle_new_session(request, state).await {
            Ok(response) => serde_json::to_value(response)?,
            Err(err) => serde_json::to_value(err)?,
        };

        Ok(Some(result))
    } else if request.method == "session/prompt" {
        let result = match handle_prompt(request, state, model, sender).await {
            Ok(()) => None,
            Err(err) => Some(serde_json::to_value(err)?),
        };

        Ok(result)
    } else {
        let response = ErrorResponse::method_not_found(request.id);
        let response = serde_json::to_value(response)?;
        Ok(Some(response))
    }
}

pub async fn handle_initialize(
    request: Request<Value>,
    state: &mut ConnectionState,
) -> Result<SuccessResponse<Value>, ErrorResponse> {
    let params = request.params.unwrap_or(Value::Null);

    if serde_json::from_value::<InitializeParams>(params).is_err() {
        let response = ErrorResponse::invalid_params(request.id);
        return Err(response);
    }

    state.initialized = true;
    let result = serde_json::json!({
        "protocolVersion": 1,
        "agentCapabilities": {},
        "authMethods": []
    });

    let response = SuccessResponse {
        jsonrpc: String::from("2.0"),
        id: request.id,
        result,
    };

    Ok(response)
}

pub async fn handle_new_session(
    request: Request<Value>,
    state: &mut ConnectionState,
) -> Result<SuccessResponse<Value>, ErrorResponse> {
    let params = request.params.unwrap_or(Value::Null);
    let NewSessionParams {
        cwd,
        mcp_servers: _,
    } = match serde_json::from_value::<NewSessionParams>(params) {
        Ok(params) => params,
        Err(_) => {
            let response = ErrorResponse::invalid_params(request.id);
            return Err(response);
        }
    };

    if !cwd.is_absolute() {
        let response = ErrorResponse::invalid_params(request.id);
        return Err(response);
    }

    let session_id = SessionId::new();
    let session_entry = Arc::new(SessionEntry::new(Session::empty(cwd)));

    {
        let mut sessions = state.sessions.write().await;
        sessions.insert(session_id.clone(), session_entry);
    }

    let result = serde_json::json!({
        "sessionId": session_id,
    });
    let response = SuccessResponse {
        jsonrpc: String::from("2.0"),
        id: request.id,
        result,
    };

    Ok(response)
}

pub async fn handle_prompt<M: Model + Send + Sync + 'static>(
    request: Request<Value>,
    state: &mut ConnectionState,
    model: &Arc<M>,
    sender: &mpsc::Sender<Value>,
) -> Result<(), ErrorResponse> {
    let value = request.params.unwrap_or(Value::Null);
    let (session_id, prompt) = match parse_prompt_params(Some(value)) {
        Ok(params) => params,
        Err(_) => {
            let response = ErrorResponse::invalid_params(request.id);
            return Err(response);
        }
    };

    return match get_session(&state.sessions, &session_id).await {
        Some(session_entry) => {
            let model = model.clone();
            let sender = sender.clone();

            tokio::spawn(async move {
                issue_prompt(
                    request.id,
                    &model,
                    &sender,
                    session_entry,
                    &prompt,
                    session_id,
                )
                .await
            });

            Ok(())
        }
        None => {
            let response = ErrorResponse::resource_not_found(request.id);
            Err(response)
        }
    };
}

fn parse_prompt_params(params: Option<Value>) -> Result<(SessionId, String), ()> {
    let PromptParams { session_id, prompt } =
        serde_json::from_value(params.unwrap_or(Value::Null)).map_err(|_| ())?;

    if prompt.is_empty() {
        return Err(());
    }

    let prompt = join_text_blocks(prompt).ok_or(())?;

    Ok((session_id, prompt))
}

fn join_text_blocks(blocks: Vec<PromptContentBlock>) -> Option<String> {
    let mut text_blocks = Vec::new();

    for block in blocks {
        match block {
            PromptContentBlock::Text { text } => text_blocks.push(text),
            PromptContentBlock::Unsupported => return None,
        }
    }

    Some(text_blocks.join("\n"))
}

pub async fn issue_prompt<M: Model + Send + Sync + 'static>(
    request_id: Value,
    model: &Arc<M>,
    sender: &mpsc::Sender<Value>,
    session_entry: Arc<SessionEntry>,
    prompt: &str,
    session_id: SessionId,
) -> Result<(), AcpError> {
    let mut session_guard = session_entry.session.lock().await;
    let history_length = session_guard.conversation.messages.len();
    let (stop_sender, stop_receiver) = oneshot::channel::<()>();

    {
        let mut turn_stop = session_entry.active_turn_stop.lock().await;
        turn_stop.replace(stop_sender);
    }

    let turn_result = tokio::select! {
        result = agent::turn(model.as_ref(), &mut session_guard, prompt) => {
                Some(result)
            }

            _ = stop_receiver => {
                None
            }
    };

    {
        // Clear the active-turn cancellation sender.
        let mut turn_stop = session_entry.active_turn_stop.lock().await;
        let _ = turn_stop.take();
    }

    match turn_result {
        Some(result) => {
            let answer = match result {
                Ok(answer) => answer,
                Err(error) => {
                    let message = format!("Request {} failed: {error}", request_id);

                    let response = ErrorResponse::internal_error(request_id, &message);
                    sender.send(serde_json::to_value(response)?).await?;

                    return Ok(());
                }
            };

            let session_update = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "session/update",
                "params": {
                    "sessionId": &session_id.0.to_string(),
                    "update": {
                        "content": {
                            "type": "text",
                            "text": answer
                        },
                        "sessionUpdate": "agent_message_chunk",
                    }
                }
            });

            sender.send(session_update).await?;

            let response = SuccessResponse {
                jsonrpc: String::from("2.0"),
                id: request_id,
                result: serde_json::json!({
                    "stopReason": "end_turn"
                }),
            };

            sender.send(serde_json::to_value(response)?).await?;

            Ok::<(), AcpError>(())
        }
        None => {
            session_guard.conversation.messages.truncate(history_length);

            let response = SuccessResponse {
                jsonrpc: String::from("2.0"),
                id: request_id,
                result: serde_json::json!({
                    "stopReason": "cancelled"
                }),
            };

            sender.send(serde_json::to_value(response)?).await?;

            Ok::<(), AcpError>(())
        }
    }
}

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use serde_json::Value;
    use tokio::sync::mpsc;
    use uuid::Uuid;

    use crate::{
        acp::{
            ConnectionState, Request,
            json_rpc::{
                CONNECTION_NOT_INITIALIZED_CODE, INVALID_PARAMS_CODE, RESOURCE_NOT_FOUND_CODE,
            },
            message_handling::{dispatch_request, join_text_blocks, parse_prompt_params},
            schema::PromptContentBlock,
            session_registry::SessionId,
        },
        agent::Model,
        test_messages::make_prompt_request,
    };

    struct InertModel;

    impl Model for InertModel {
        async fn complete(
            &self,
            _request: Value,
        ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
            panic!("Should not be called")
        }
    }

    #[tokio::test]
    async fn session_creation_before_initialization_is_rejected() {
        let request = Request {
            id: serde_json::json!(1),
            method: String::from("session/new"),
            params: Some(serde_json::json!({
                "cwd": "/tmp",
                "mcpServers": [],
            })),
        };

        let mut state = ConnectionState::new();
        let model = Arc::new(InertModel);
        let (sender, _) = mpsc::channel(2);

        let response = dispatch_request(request, &mut state, &model, &sender)
            .await
            .expect("dispatch should succeed")
            .expect("request should produce a response");

        assert_eq!(response["id"], serde_json::json!(1));
        assert_eq!(
            response["error"]["code"],
            serde_json::json!(CONNECTION_NOT_INITIALIZED_CODE)
        );
        assert!(response.get("result").is_none());
        assert!(state.sessions.read().await.is_empty());
    }

    #[tokio::test]
    async fn session_creation_rejects_malformed_mcp_server_field() {
        let request = Request {
            id: serde_json::json!(1),
            method: String::from("session/new"),
            params: Some(serde_json::json!({
                "cwd": "/tmp",
                "mcpServers": "bogus",
            })),
        };

        let mut state = ConnectionState::new();
        state.initialized = true;
        let model = Arc::new(InertModel);
        let (sender, _) = mpsc::channel(2);

        let response = dispatch_request(request, &mut state, &model, &sender)
            .await
            .expect("dispatch should succeed")
            .expect("should return a response with an error field");

        {
            let registry = state.sessions.read().await;
            assert_eq!(registry.len(), 0);
        }

        assert_eq!(response["id"], serde_json::json!(1));
        assert_eq!(
            response["error"]["code"],
            serde_json::json!(INVALID_PARAMS_CODE)
        );
        assert!(response.get("result").is_none());
    }

    #[tokio::test]
    async fn session_creation_rejects_relative_working_directory() {
        let request = Request {
            id: serde_json::json!(1),
            method: String::from("session/new"),
            params: Some(serde_json::json!({
                "cwd": "relative/path",
                "mcpServers": [],
            })),
        };

        let mut state = ConnectionState::new();
        state.initialized = true;

        let model = Arc::new(InertModel);
        let (sender, _) = mpsc::channel(2);

        let response = dispatch_request(request, &mut state, &model, &sender)
            .await
            .expect("dispatch should succeed")
            .expect("should return a response with an error field");

        {
            let registry = state.sessions.read().await;
            assert_eq!(registry.len(), 0);
        }

        assert_eq!(response["id"], serde_json::json!(1));
        assert_eq!(
            response["error"]["code"],
            serde_json::json!(INVALID_PARAMS_CODE)
        );
        assert!(response.get("result").is_none());
    }

    #[tokio::test]
    async fn session_creation_returns_distinct_independent_sessions() {
        let mut state = ConnectionState::new();
        state.initialized = true;

        fn make_request(request_id: u64, cwd: String) -> Request<Value> {
            Request {
                id: serde_json::json!(request_id),
                method: String::from("session/new"),
                params: Some(serde_json::json!({
                    "cwd": cwd,
                    "mcpServers": [],
                })),
            }
        }

        let first_request = make_request(1, "/tmp/session_one".to_string());
        let second_request = make_request(2, "/tmp/session_two".to_string());

        let model = Arc::new(InertModel);
        let (sender, _) = mpsc::channel(2);

        let first_response = dispatch_request(first_request, &mut state, &model, &sender)
            .await
            .expect("dispatch should succeed")
            .expect("request should produce a response");

        let second_response = dispatch_request(second_request, &mut state, &model, &sender)
            .await
            .expect("dispatch should succeed")
            .expect("request should produce a response");

        assert_eq!(first_response["id"], serde_json::json!(1));
        assert_eq!(second_response["id"], serde_json::json!(2));

        let first_session_id: SessionId =
            serde_json::from_value(first_response["result"]["sessionId"].clone())
                .expect("session ID should be a valid UUID");
        let second_session_id: SessionId =
            serde_json::from_value(second_response["result"]["sessionId"].clone())
                .expect("session ID should be a valid UUID");

        assert_ne!(first_session_id, second_session_id);

        let (first_session_entry, second_session_entry) = {
            let registry = state.sessions.read().await;

            assert_eq!(registry.len(), 2);

            (
                registry
                    .get(&first_session_id)
                    .cloned()
                    .expect("returned session ID should exist in the registry"),
                registry
                    .get(&second_session_id)
                    .cloned()
                    .expect("returned session ID should exist in the registry"),
            )
        };

        {
            let first_session = first_session_entry.session.lock().await;
            assert_eq!(
                first_session.cwd(),
                std::path::Path::new("/tmp/session_one")
            );
            assert!(first_response.get("error").is_none());
        }

        {
            let second_session = second_session_entry.session.lock().await;

            assert_eq!(
                second_session.cwd(),
                std::path::Path::new("/tmp/session_two")
            );
            assert!(second_response.get("error").is_none());
        }
    }

    #[tokio::test]
    async fn session_creation_returns_id_and_retains_working_directory() {
        let request = Request {
            id: serde_json::json!(1),
            method: String::from("session/new"),
            params: Some(serde_json::json!({
                "cwd": "/tmp",
                "mcpServers": [],
            })),
        };

        let mut state = ConnectionState::new();
        state.initialized = true;
        let model = Arc::new(InertModel);
        let (sender, _) = mpsc::channel(2);

        let response = dispatch_request(request, &mut state, &model, &sender)
            .await
            .expect("dispatch should succeed")
            .expect("request should produce a response");

        assert_eq!(response["id"], serde_json::json!(1));
        let session_id: SessionId = serde_json::from_value(response["result"]["sessionId"].clone())
            .expect("session ID should be a valid UUID");

        let session_entry = {
            let registry = state.sessions.read().await;

            registry
                .get(&session_id)
                .cloned()
                .expect("returned session ID should exist in the registry")
        };

        let session = session_entry.session.lock().await;

        assert_eq!(session.cwd(), std::path::Path::new("/tmp"));
        assert!(response.get("error").is_none());
    }

    #[tokio::test]
    async fn session_prompt_request_rejected_with_malformed_prompt_parameters() {
        let mut state = ConnectionState::new();
        state.initialized = true;

        let value = serde_json::json!({
          "jsonrpc": "2.0",
          "id": 3,
          "method": "session/prompt",
          "params": {
              "sessionId": "00000000-0000-0000-0000-000000000001",
              "prompt": []
          }
        });
        let request: Request<Value> = serde_json::from_value(value).expect("to parse as JSON-RPC");
        let model = Arc::new(InertModel);
        let (sender, _receiver) = mpsc::channel(2);

        let result = dispatch_request(request, &mut state, &model, &sender)
            .await
            .expect("dispatch should succeed")
            .expect("request should produce an error response");

        assert_eq!(result["id"], serde_json::json!(3));
        assert!(
            result.get("result").is_none(),
            "an error response must not contain a result"
        );
        assert_eq!(result["error"]["code"], INVALID_PARAMS_CODE);
    }

    #[tokio::test]
    async fn session_prompt_request_rejected_with_missing_session() {
        let mut state = ConnectionState::new();
        state.initialized = true;

        let session_id = "00000000-0000-0000-0000-000000000001";
        let value = make_prompt_request(3, session_id, "What is 1+1?");
        let request: Request<Value> = serde_json::from_value(value).expect("to parse as JSON-RPC");
        let model = Arc::new(InertModel);
        let (sender, _receiver) = mpsc::channel(2);

        let result = dispatch_request(request, &mut state, &model, &sender)
            .await
            .expect("dispatch should succeed")
            .expect("request should produce an error response");

        assert_eq!(result["id"], serde_json::json!(3));
        assert!(
            result.get("result").is_none(),
            "an error response must not contain a result"
        );
        assert_eq!(result["error"]["code"], RESOURCE_NOT_FOUND_CODE);
    }

    #[test]
    fn join_text_blocks_joins_text_with_a_newline() {
        let blocks = vec![
            PromptContentBlock::Text {
                text: "first".to_string(),
            },
            PromptContentBlock::Text {
                text: "second".to_string(),
            },
        ];

        let joined = join_text_blocks(blocks).expect("one joined text");
        assert_eq!("first\nsecond".to_string(), joined);
    }

    #[test]
    fn join_text_blocks_rejects_unsupported_blocks() {
        let blocks = vec![
            PromptContentBlock::Text {
                text: "text".to_string(),
            },
            PromptContentBlock::Unsupported,
        ];

        let joined = join_text_blocks(blocks);
        assert_eq!(None, joined);
    }

    #[test]
    fn parse_prompt_params_when_the_structure_is_valid() {
        let expected = "00000000-0000-0000-0000-000000000001";
        let params = serde_json::json!({
            "sessionId": &expected.to_string(),
            "prompt": [
                { "type": "text", "text": "Hello"}
            ]
        });

        let (session_id, prompt) =
            parse_prompt_params(Some(params)).expect("valid prompt params should parse");
        assert_eq!(
            session_id.0,
            Uuid::parse_str(expected).expect("expected ID to be a valid UUID")
        );

        assert_eq!(prompt, "Hello");
    }

    #[test]
    fn parse_prompt_params_rejects_unsupported_blocks() {
        let expected = "00000000-0000-0000-0000-000000000001";
        let params = serde_json::json!({
            "sessionId": &expected.to_string(),
            "prompt": [
                { "type": "image" }
            ]
        });

        parse_prompt_params(Some(params)).expect_err("unsupported blocks should not parse");
    }

    #[test]
    fn parse_prompt_params_rejects_malformed_parameters() {
        let params = serde_json::json!({
            "prompt": [
                { "type": "text", "text": "Hello" }
            ]
        });

        parse_prompt_params(Some(params)).expect_err("malformed parameters should not parse");
    }

    #[test]
    fn parse_prompt_params_rejects_empty_prompt_array() {
        let expected = "00000000-0000-0000-0000-000000000001";
        let params = serde_json::json!({
            "sessionId": expected.to_string(),
            "prompt": []
        });

        parse_prompt_params(Some(params)).expect_err("empty prompt arrays should not parse");
    }
}
