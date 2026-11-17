use std::time::Duration;

use serde_json::Value;
use tokio::io::{
    AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, ReadHalf, WriteHalf, duplex,
};

use crate::acp::AcpError;

pub(super) struct TestClient {
    pub client_reader: BufReader<ReadHalf<DuplexStream>>,
    pub client_writer: WriteHalf<DuplexStream>,
    pub agent_end: Option<DuplexStream>,
}

impl TestClient {
    pub fn new() -> Self {
        let (client_end, agent_end) = duplex(1024);
        let (client_read, client_writer) = tokio::io::split(client_end);

        let client_reader = tokio::io::BufReader::new(client_read);
        let client_writer = client_writer;

        Self {
            client_reader,
            client_writer,
            agent_end: Some(agent_end),
        }
    }

    pub async fn initialize(&mut self) {
        let initialize_request = serde_json::json!({
          "jsonrpc": "2.0",
          "id": 1,
          "method": "initialize",
          "params": {
              "protocolVersion": 1,
          }
        });

        self.send_frame(&initialize_request).await;

        let response = self.read_frame().await;

        TestClient::validate_version(&response).expect("response should use JSON-RPC 2.0");
        assert_eq!(response["id"], serde_json::json!(1));
        assert!(response["error"].is_null());
        assert_eq!(response["result"]["protocolVersion"], serde_json::json!(1));
    }

    async fn send_frame(&mut self, data: &Value) {
        let mut frame = serde_json::to_string(data).unwrap();
        frame.push('\n');

        self.client_writer
            .write_all(frame.as_bytes())
            .await
            .expect("frame should serialize");
    }

    async fn read_frame(&mut self) -> Value {
        tokio::time::timeout(Duration::from_secs(1), async {
            let mut received = String::new();
            match self.client_reader.read_line(&mut received).await {
                Ok(0) => panic!("Received EOF instead of a complete frame"),
                Ok(_) => {
                    assert!(received.ends_with("\n"));
                    serde_json::from_str(&received).expect("frame should be valid JSON")
                }
                Err(error) => panic!("Unable to read a frame: {}", error),
            }
        })
        .await
        .expect("to read a JSON RPC frame")
    }

    fn validate_version(value: &Value) -> Result<(), AcpError> {
        match value.get("jsonrpc").and_then(|version| version.as_str()) {
            Some("2.0") => Ok(()),
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "expected JSON-RPC version 2.0",
            )
            .into()),
        }
    }
}
