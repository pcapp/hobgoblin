use tokio::io::{BufReader, DuplexStream, ReadHalf, WriteHalf, duplex};

pub(super) struct TestClient {
    pub client_reader: BufReader<ReadHalf<DuplexStream>>,
    pub client_writer: WriteHalf<DuplexStream>,
    pub agent_end: DuplexStream,
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
            agent_end,
        }
    }
}
