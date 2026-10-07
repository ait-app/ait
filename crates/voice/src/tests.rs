use super::*;

#[test]
fn errors_have_safe_stable_codes_and_capabilities_are_installed_only_with_service() {
    assert!(capabilities::installed_capabilities(false).next().is_none());
    assert_eq!(
        capabilities::installed_capabilities(true).collect::<Vec<_>>(),
        connection::METHODS
            .iter()
            .map(|method| method.name)
            .collect::<Vec<_>>()
    );
    for error in [
        Error::Invalid,
        Error::Unavailable,
        Error::Preparing,
        Error::ModelDownload,
        Error::Capacity,
        Error::Timeout,
        Error::Provider,
        Error::Cancelled,
        Error::Agent,
    ] {
        assert!(!error.reason().is_empty());
        assert!(!error.to_string().contains('/'));
        let code = model::ErrorCode::from(error);
        assert!(!code.message().is_empty());
        assert_eq!(
            error.retryable(),
            matches!(
                error,
                Error::Capacity
                    | Error::Timeout
                    | Error::Provider
                    | Error::Preparing
                    | Error::ModelDownload
            )
        );
    }
}
