//! `DeepSeek` Harness launcher for the shared bounded ACP transport.

use super::DeepSeekHarnessClient;
use crate::local::acp_transport::Transport;
use provider_core::ports::agent_session::AgentSessionError;

pub(super) fn spawn(
    client: &DeepSeekHarnessClient,
    cwd: &str,
) -> Result<Transport, AgentSessionError> {
    let mut command = client.command();
    command.args(["--profile", "acp"]);
    Transport::spawn(command, cwd, client.deadline, "deepseek-harness")
}
