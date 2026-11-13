use std::sync::Arc;

use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

use crate::{
    acp::{
        AcpError, ConnectionState, ErrorResponse, INTERNAL_ERROR_CODE, InitializeParams,
        NewSessionParams, PromptContentBlock, PromptParams, Request, SuccessResponse,
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

                    let response = ErrorResponse::new(request_id, INTERNAL_ERROR_CODE, &message);
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
    use uuid::Uuid;

    use crate::acp::{
        PromptContentBlock,
        message_handling::{join_text_blocks, parse_prompt_params},
    };

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
