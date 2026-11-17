use super::ErrorCode;

#[test]
fn converts_model_errors_to_filesystem_errors() {
    assert_eq!(
        ErrorCode::from(model::ErrorCode::InvalidMessage),
        ErrorCode::InvalidMessage
    );
    assert_eq!(
        ErrorCode::from(model::ErrorCode::MethodNotFound),
        ErrorCode::MethodNotFound
    );
    assert_eq!(
        ErrorCode::from(model::ErrorCode::ProjectIo),
        ErrorCode::ProjectIo
    );
    assert_eq!(
        ErrorCode::from(model::ErrorCode::ResourceExhausted),
        ErrorCode::ResourceExhausted
    );
    assert_eq!(
        ErrorCode::from(model::ErrorCode::DaemonIo),
        ErrorCode::ProjectIo
    );
}
