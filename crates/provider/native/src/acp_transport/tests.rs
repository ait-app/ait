use tokio::process::Command;

use super::*;

fn transport(script: &str) -> Transport {
    let mut command = Command::new("python3");
    command.args(["-c", script]);
    Transport::spawn(command, ".", Duration::from_secs(3), "test").unwrap()
}

#[tokio::test]
async fn a_final_frame_without_a_newline_is_delivered_before_closed() {
    let mut transport = transport(
        r#"import sys
print('{"jsonrpc":"2.0","method":"first"}')
sys.stdout.write('{"jsonrpc":"2.0","method":"last"}')
"#,
    );
    assert_eq!(transport.receive().await.unwrap()["method"], "first");
    assert_eq!(transport.receive().await.unwrap()["method"], "last");
    assert_eq!(transport.receive().await, Err(AgentSessionError::Failed));
    assert_eq!(
        transport.diagnostics.failure().unwrap().category,
        Category::Closed
    );
    transport.close().await.unwrap();
}

#[tokio::test]
async fn a_frame_at_the_read_limit_is_not_treated_as_eof() {
    let mut transport = transport(&format!(
        r#"import sys
frame = '{{"jsonrpc":"2.0","method":"oversized"}}'
sys.stdout.write(frame.ljust({MAX_FRAME}) + '\n{{"jsonrpc":"2.0","method":"next"}}\n')
"#,
    ));
    assert_eq!(transport.receive().await, Err(AgentSessionError::Failed));
    assert_eq!(
        transport.diagnostics.failure().unwrap().category,
        Category::Protocol
    );
    transport.close().await.unwrap();
}
