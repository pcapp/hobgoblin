use serde_json::Value;
use tokio::{
    io::{AsyncWrite, AsyncWriteExt},
    sync::mpsc,
};

use crate::acp::AcpError;

pub async fn write_frame<W: AsyncWrite + Unpin>(
    writer: &mut W,
    value: &Value,
) -> Result<(), AcpError> {
    let mut frame = serde_json::to_vec(value)?;
    frame.push(b'\n');

    writer.write_all(&frame).await?;
    writer.flush().await?;

    Ok(())
}

pub async fn write_frames<W: AsyncWrite + Unpin + Send>(
    mut writer: W,
    mut receiver: mpsc::Receiver<Value>,
) -> Result<(), AcpError> {
    while let Some(value) = receiver.recv().await {
        write_frame(&mut writer, &value).await?;
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use tokio::io::{AsyncReadExt, BufWriter, duplex};

    use crate::acp::transport::write_frame;

    #[tokio::test]
    async fn writes_one_compact_newline_terminated_frame_and_flushes() {
        let value = serde_json::json!({
            "result": {
                "protocolVersion": 1
            }
        });

        let (mut client_end, agent_end) = duplex(1024);

        let mut agent_output = BufWriter::new(agent_end);

        write_frame(&mut agent_output, &value)
            .await
            .expect("frame should be written successfully.");
        drop(agent_output);

        let mut received = String::new();
        client_end
            .read_to_string(&mut received)
            .await
            .expect("client should read the frame");

        assert_eq!(received, "{\"result\":{\"protocolVersion\":1}}\n");
    }
}
