use reqwest::header;

use super::Api;
use crate::ports::agent_session::AgentSessionError;

impl Api {
    /// Request the complete native session tree through the authenticated binary export route.
    /// Rejects non-ZIP or unsuccessful responses without exposing the URL or response body.
    pub(in crate::local::deepseek_harness::native) async fn export_session(
        &self,
        session: &str,
    ) -> Result<reqwest::Response, AgentSessionError> {
        let mut url = self
            .base
            .join("api/session.export")
            .map_err(|_| AgentSessionError::Failed)?;
        url.query_pairs_mut()
            .append_pair("sessionId", session)
            .append_pair("includeDescendants", "true");
        let response = self
            .client
            .get(url)
            .header(header::COOKIE, self.cookie.clone())
            .send()
            .await
            .map_err(|error| self.transport_error(&error, "session_export", "http_send"))?;
        if !response.status().is_success()
            || response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.split(';').next())
                != Some("application/zip")
        {
            return Err(AgentSessionError::Failed);
        }
        Ok(response)
    }
}
