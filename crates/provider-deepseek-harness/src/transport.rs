//! `DeepSeek` Harness launcher for the shared bounded ACP transport.

use super::DeepSeekHarnessClient;
use provider_core::ports::agent_session::AgentSessionError;
use provider_native::acp_transport::Transport;

pub(super) fn spawn(
    client: &DeepSeekHarnessClient,
    cwd: &str,
) -> Result<Transport, AgentSessionError> {
    let mut command = client.command();
    command.args(["--profile", "acp"]);
    Transport::spawn(command, cwd, client.deadline, "deepseek-harness")
}
