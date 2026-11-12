use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use serde_json::Value;
use tokio::sync::Notify;

use crate::{
    agent::Model,
    scripted_model::TurnResponse::{Done, TimeGated},
};
pub(crate) enum TurnResponse {
    Done {
        response: Value,
    },
    TimeGated {
        response: Value,
        started: Arc<Notify>,
        released: Arc<Notify>,
    },
}

pub(crate) struct ScriptedModel {
    pub requests: Mutex<Vec<Value>>,
    // Canned, expected responses
    pub responses: Mutex<VecDeque<TurnResponse>>,
}

impl ScriptedModel {
    pub fn new(responses: VecDeque<TurnResponse>) -> Self {
        Self {
            responses: Mutex::new(responses),
            requests: Mutex::new(vec![]),
        }
    }
}

impl Model for ScriptedModel {
    async fn complete(
        &self,
        request: Value,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        {
            let mut guard = self.requests.lock().expect("to get the lock");
            guard.push(request);
        }

        let response = {
            let mut guard = self.responses.lock().expect("to get the lock");
            guard.pop_front().expect("to have a response")
        };

        match response {
            Done { response } => Ok(response),
            TimeGated {
                response,
                started,
                released,
            } => {
                started.notify_one();
                released.notified().await;

                Ok(response)
            }
        }
    }
}

impl Model for Arc<ScriptedModel> {
    fn complete(
        &self,
        request: Value,
    ) -> impl std::future::Future<Output = Result<Value, Box<dyn std::error::Error + Send + Sync>>> + Send
    {
        self.as_ref().complete(request)
    }
}

#[tokio::test]
async fn test_scripted_model_with_immediate_response() {
    let model_response = serde_json::json!({
          "choices": [{
            "message": {
              "role": "assistant",
              "content": "scripted answer",
              "tool_calls": null
            }
          }]
    });

    let model = ScriptedModel::new(VecDeque::from([TurnResponse::Done {
        response: model_response.clone(),
    }]));
    let request = serde_json::json!({
        "messages": ["what is 1 + 1?"],
        "model": "hobgoblin/test",
        "tools": [],
    });

    let response = model.complete(request).await.expect("a result");

    assert_eq!(model_response, response);
}

#[tokio::test]
async fn test_scripted_model_with_time_coordination() {
    let started = Arc::new(Notify::new());
    let released = Arc::new(Notify::new());

    let model_response = serde_json::json!({
          "choices": [{
            "message": {
              "role": "assistant",
              "content": "scripted answer",
              "tool_calls": []
            }
          }]
    });

    let model = Arc::new(ScriptedModel::new(VecDeque::from([
        TurnResponse::TimeGated {
            response: model_response.clone(),
            started: started.clone(),
            released: released.clone(),
        },
    ])));

    let request = serde_json::json!({
        "messages": ["what is 1 + 1?"],
        "model": "hobgoblin/test",
        "tools": [],
    });

    let model_clone = model.clone();
    let task = tokio::spawn(async move { model_clone.complete(request).await });

    started.notified().await;

    released.notify_one();
    let response = task
        .await
        .expect("the task to complete")
        .expect("an Ok result");
    {
        let guard = model.requests.lock().unwrap();
        assert_eq!(guard.len(), 1);
    }

    assert_eq!(model_response, response);
}
