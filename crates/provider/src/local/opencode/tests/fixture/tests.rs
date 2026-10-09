use super::*;
use crate::local::opencode::runtime::Runtime;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn concurrent_fixture_launches_keep_protocols_and_configuration_isolated() {
    let (first, second) = tokio::join!(Fixture::start(Version::V1), Fixture::start(Version::V2));
    let first_cancel = CancellationToken::new();
    let second_cancel = CancellationToken::new();
    for _ in 0..16 {
        let (first_runtime, second_runtime) = tokio::join!(
            Runtime::spawn(&first.binary, &first.cwd, &first_cancel),
            Runtime::spawn(&second.binary, &second.cwd, &second_cancel),
        );
        let mut first_runtime = first_runtime.unwrap();
        let mut second_runtime = second_runtime.unwrap();
        assert_eq!(first_runtime.api.path("ses_one", ""), "/session/ses_one");
        assert_eq!(
            second_runtime.api.path("ses_one", ""),
            "/api/session/ses_one"
        );
        let (first_closed, second_closed) =
            tokio::join!(first_runtime.close(), second_runtime.close());
        first_closed.unwrap();
        second_closed.unwrap();
    }
}
