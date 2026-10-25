use std::io;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};

use super::engine::Engine;

const MAX_FRAME: usize = 64 * 1024 * 1024;

/// Pump bounded JSONL from `input` to `output` using `engine`; always close the native owner.
/// Returns parse/pipe/ownership failures without replaying any submitted input.
pub(super) async fn serve<R, W>(mut engine: Engine, input: R, mut output: W) -> io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let result = pump(&mut engine, input, &mut output).await;
    let closed = engine
        .close()
        .await
        .map_err(|error| io::Error::other(error.to_string()));
    result.and(closed)
}

async fn pump<R, W>(engine: &mut Engine, input: R, output: &mut W) -> io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut input = BufReader::new(input);
    let mut bytes = Vec::new();
    let mut tick = tokio::time::interval(Duration::from_millis(5));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        let mut limited = (&mut input).take((MAX_FRAME + 1 - bytes.len()) as u64);
        let messages = tokio::select! {
            read=limited.read_until(b'\n',&mut bytes)=>{
                match read {
                    Err(error)=>break Err(error),
                    Ok(0) if bytes.is_empty()=>break Ok(()),
                    Ok(_) if bytes.len()>MAX_FRAME || bytes.last()!=Some(&b'\n')=>break Err(io::Error::other("Invalid or oversized ACP frame")),
                    Ok(_)=>{}
                }
                if bytes.iter().all(u8::is_ascii_whitespace) {bytes.clear();continue;}
                let parsed=serde_json::from_slice::<Value>(&bytes);
                bytes.clear();
                match parsed {
                    Ok(message)=>engine.dispatch(message).await,
                    Err(_)=>vec![json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Parse error"}})]
                }
            },
            _=tick.tick()=>match engine.poll() {
                Ok(messages)=>messages,
                Err(error)=>break Err(io::Error::other(error.to_string())),
            }
        };
        for message in messages {
            let Ok(mut encoded) = serde_json::to_vec(&message) else {
                return Err(io::Error::other("Cannot encode ACP response"));
            };
            if encoded.len() >= MAX_FRAME {
                return Err(io::Error::other("ACP response exceeded limit"));
            }
            encoded.push(b'\n');
            output.write_all(&encoded).await?;
        }
        if let Err(error) = output.flush().await {
            break Err(error);
        }
    }
}
