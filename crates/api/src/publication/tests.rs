use super::*;

#[test]
fn only_transient_failures_are_retryable_and_routes_agree() {
    for (error, status, code) in [
        (
            host_link::Error::Protocol,
            StatusCode::BAD_REQUEST,
            ErrorCode::InvalidMessage,
        ),
        (
            host_link::Error::Unauthorized,
            StatusCode::CONFLICT,
            ErrorCode::Forbidden,
        ),
        (
            host_link::Error::Denied,
            StatusCode::CONFLICT,
            ErrorCode::Forbidden,
        ),
        (
            host_link::Error::Conflict,
            StatusCode::CONFLICT,
            ErrorCode::Forbidden,
        ),
        (
            host_link::Error::Storage,
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::DaemonIo,
        ),
        (
            host_link::Error::Unavailable,
            StatusCode::SERVICE_UNAVAILABLE,
            ErrorCode::DaemonIo,
        ),
    ] {
        assert_eq!(classify(error), (status, code), "{error:?}");
        assert_eq!(http_error(error).0, status, "{error:?}");
        assert_eq!(
            code.retryable(),
            matches!(code, ErrorCode::DaemonIo),
            "{error:?}"
        );
        assert_ne!(code, ErrorCode::AgentIo);
    }
}
