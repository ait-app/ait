use super::*;

fn transport(script: &str) -> Transport {
    let client = AntigravityClient::new("python3".into());
    Transport::spawn(&client, ".", &["-c".to_owned(), script.to_owned()]).unwrap()
}

#[tokio::test]
async fn a_final_frame_without_a_newline_is_delivered_before_closed() {
    let mut transport = transport(
        r#"import sys
print('{"event":"first"}')
sys.stdout.write('{"event":"last"}')
"#,
    );
    assert_eq!(transport.receive().await.unwrap()["event"], "first");
    assert_eq!(transport.receive().await.unwrap()["event"], "last");
    assert_eq!(transport.receive().await, Err(AgentSessionError::Failed));
    assert_eq!(transport.failure(), Some(Failure::Exit));
    transport.close().await.unwrap();
}

#[tokio::test]
async fn a_frame_at_the_read_limit_is_not_treated_as_eof() {
    let mut transport = transport(&format!(
        r#"import sys
frame = '{{"event":"oversized"}}'
sys.stdout.write(frame.ljust({MAX_FRAME}) + '\n{{"event":"next"}}\n')
"#,
    ));
    assert_eq!(transport.receive().await, Err(AgentSessionError::Failed));
    assert_eq!(transport.failure(), Some(Failure::Protocol));
    transport.close().await.unwrap();
}
