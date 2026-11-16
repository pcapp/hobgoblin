#[derive(Debug)]
pub enum RequestError {
    InvalidParams,
}

impl From<serde_json::Error> for RequestError {
    fn from(_value: serde_json::Error) -> Self {
        RequestError::InvalidParams
    }
}
