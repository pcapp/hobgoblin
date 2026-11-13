use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::acp::AcpError;

pub const INVALID_PARAMS_CODE: i32 = -32602;
pub const CONNECTION_NOT_INITIALIZED_CODE: i32 = -32600;
pub const METHOD_NOT_FOUND_CODE: i32 = -32601;
pub const RESOURCE_NOT_FOUND_CODE: i32 = -32002;
pub const INTERNAL_ERROR_CODE: i32 = -32603;

#[derive(Debug, Deserialize)]
pub struct Notification<T> {
    pub method: String,
    pub params: Option<T>,
}

#[derive(Debug, Deserialize)]
pub struct Request<T> {
    pub id: Value,
    pub method: String,
    pub params: Option<T>,
}

#[derive(Debug, Serialize)]
pub struct SuccessResponse<T> {
    pub jsonrpc: String,
    pub id: Value,
    pub result: T,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    jsonrpc: String,
    id: Value,
    error: ResponseError,
}

impl ErrorResponse {
    pub fn new(id: Value, code: i32, message: &str) -> Self {
        Self {
            jsonrpc: String::from("2.0"),
            id,
            error: ResponseError {
                code,
                message: message.to_string(),
            },
        }
    }
    pub fn invalid_params(id: Value) -> Self {
        Self::new(id, INVALID_PARAMS_CODE, "Invalid params")
    }

    pub fn connection_not_initialized(id: Value) -> Self {
        Self::new(
            id,
            CONNECTION_NOT_INITIALIZED_CODE,
            "Connection not initialized",
        )
    }

    pub fn method_not_found(id: Value) -> Self {
        Self::new(id, METHOD_NOT_FOUND_CODE, "Method not found")
    }

    pub fn resource_not_found(id: Value) -> Self {
        Self::new(id, RESOURCE_NOT_FOUND_CODE, "Resource not found")
    }

    pub fn internal_error(id: Value, message: &str) -> Self {
        Self::new(id, INTERNAL_ERROR_CODE, message)
    }
}

#[derive(Debug, Serialize)]
pub struct ResponseError {
    pub code: i32,
    pub message: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum MessageKind {
    Request,
    Notification,
    Response,
}

pub fn classify_message(value: &Value) -> Result<MessageKind, AcpError> {
    let object = value.as_object().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "JSON-RPC message must be an object.",
        )
    })?;

    let has_method = object.contains_key("method");
    let has_id = object.contains_key("id");
    let has_result = object.contains_key("result");
    let has_error = object.contains_key("error");

    match (has_method, has_id, has_result, has_error) {
        (true, true, false, false) => Ok(MessageKind::Request),
        (true, false, false, false) => Ok(MessageKind::Notification),
        (false, true, true, false) | (false, true, false, true) => Ok(MessageKind::Response),
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid JSON-RPC message shape",
        )
        .into()),
    }
}

pub fn validate_version(value: &Value) -> Result<(), AcpError> {
    match value.get("jsonrpc").and_then(|version| version.as_str()) {
        Some("2.0") => Ok(()),
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "expected JSON-RPC version 2.0",
        )
        .into()),
    }
}

#[cfg(test)]
mod test {
    use crate::acp::json_rpc::{MessageKind, classify_message, validate_version};

    #[test]
    fn classify_request() {
        let value = serde_json::json!({
            "id": 1,
            "method": "initialize",
            "params": {},
        });

        let kind = classify_message(&value).expect("to be a valid Request");

        assert_eq!(kind, MessageKind::Request);
    }

    #[test]
    fn classify_notification() {
        let value = serde_json::json!({
            "method": "method_name",
            "params": {},
        });

        let kind = classify_message(&value).expect("to be a valid Notification");

        assert_eq!(kind, MessageKind::Notification);
    }

    #[test]
    fn classify_successful_response() {
        let value = serde_json::json!({
            "id": 1,
            "result": {}
        });

        let kind = classify_message(&value).expect("to be a valid Response");

        assert_eq!(kind, MessageKind::Response);
    }

    #[test]
    fn classify_error_response() {
        let value = serde_json::json!({
            "id": 1,
            "error": {}
        });

        let kind = classify_message(&value).expect("to be a valid Response");

        assert_eq!(kind, MessageKind::Response);
    }

    #[test]
    fn classify_result_and_error_as_invalid() {
        let value = serde_json::json!({
            "id": 1,
            "error": {},
            "result": {}
        });

        let error = classify_message(&value)
            .expect_err("a message with a result and an error should be rejected");

        assert_eq!(error.to_string(), "invalid JSON-RPC message shape");
    }

    #[test]
    fn classify_method_and_result_as_invalid() {
        let value = serde_json::json!({
            "id": 1,
            "method": "message",
            "result": {}
        });

        let error = classify_message(&value)
            .expect_err("a message with a method and a result should be rejected");

        assert_eq!(error.to_string(), "invalid JSON-RPC message shape");
    }

    #[test]
    fn classify_method_and_error_as_invalid() {
        let value = serde_json::json!({
            "id": 1,
            "method": "message",
            "error": {}
        });

        let error = classify_message(&value)
            .expect_err("a message with a method and an error should be rejected");

        assert_eq!(error.to_string(), "invalid JSON-RPC message shape");
    }

    #[test]
    fn classify_result_without_id_as_invalid() {
        let value = serde_json::json!({
            "result": {}
        });

        let error = classify_message(&value)
            .expect_err("a response-shaped message without an ID should be rejected");

        assert_eq!(error.to_string(), "invalid JSON-RPC message shape");
    }

    #[test]
    fn classify_empty_object_as_invalid() {
        let value = serde_json::json!({});

        let error = classify_message(&value)
            .expect_err("an object without identifying fields should be rejected");

        assert_eq!(error.to_string(), "invalid JSON-RPC message shape");
    }

    #[test]
    fn classify_non_object_as_invalid() {
        let value = serde_json::json!([]);

        let error = classify_message(&value).expect_err("a JSON-RPC message must be an object");

        assert_eq!(error.to_string(), "JSON-RPC message must be an object.");
    }

    #[test]
    fn invalidates_jsonrpc_other_than_2() {
        let request = serde_json::json!({
            "jsonrpc": "1.0",
            "id": 0,
            "method": "initialize",
        });

        let error = validate_version(&request)
            .expect_err("JSON-RPC versions other than 2.0 should be rejected");

        assert_eq!(error.to_string(), "expected JSON-RPC version 2.0");
    }
}
