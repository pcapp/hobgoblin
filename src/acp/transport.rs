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
