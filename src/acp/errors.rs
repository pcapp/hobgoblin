#[derive(Debug)]
pub enum RequestError {
    ConnectionNotInitialized,
    InvalidParams,
    ResourceNotFound,
    MethodNotFound,
    InternalError { message: String },
}

impl From<serde_json::Error> for RequestError {
    fn from(_value: serde_json::Error) -> Self {
        RequestError::InvalidParams
    }
}
