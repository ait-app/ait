use std::io::Write;

use provider_core::ports::agent_session::AgentSessionError;

/// Ask the external OMP to persist sessions before a Goal can start without a user turn.
/// Returns an I/O failure if its private extension cannot be prepared.
pub(super) fn extension() -> Result<tempfile::NamedTempFile, AgentSessionError> {
    let mut file = tempfile::Builder::new()
        .prefix("ait-omp-session-")
        .suffix(".js")
        .tempfile()
        .map_err(|_| AgentSessionError::Failed)?;
    file.write_all(include_bytes!("persistence.ts"))
        .map_err(|_| AgentSessionError::Failed)?;
    file.flush().map_err(|_| AgentSessionError::Failed)?;
    Ok(file)
}
