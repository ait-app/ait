use super::{ErrorCode, ExecutionState, ResumeRequest, decode, map_manager, only};
use serde_json::{Value, json};

impl ExecutionState {
    pub(super) async fn resume(&mut self, params: Value) -> Result<Value, ErrorCode> {
        only(&params, &["handle", "overrides"])?;
        if let Some(overrides) = params.get("overrides") {
            only(
                overrides,
                &[
                    "provider",
                    "cwd",
                    "title",
                    "modeId",
                    "model",
                    "thinkingOptionId",
                    "systemPrompt",
                    "featureValues",
                    "providerOptions",
                    "mcpServers",
                    "toolPolicy",
                ],
            )?;
        }
        let mut request: ResumeRequest = decode(params)?;
        let records = self.registry.list().map_err(|_| ErrorCode::AgentIo)?;
        let mut matching = records.iter().filter(|record| {
            record.persistence.as_ref().is_some_and(|handle| {
                handle.provider == request.handle.provider
                    && handle.session_id == request.handle.session_id
            })
        });
        let record = matching.next();
        if matching.next().is_some() {
            return Err(ErrorCode::InvalidMessage);
        }
        let id = if let Some(record) = record {
            if let Some(cwd) = &request.overrides.cwd {
                request.overrides.cwd = Some(super::placement::directory(cwd)?);
            }
            if record.archived_at.is_none() {
                self.workspace(record.workspace_id.as_deref(), &record.cwd)?;
            }
            self.manager
                .restore(&record.id, &request.overrides)
                .await
                .map_err(|error| map_manager(&error))?;
            record.id.clone()
        } else {
            self.resume_unknown(request).await?
        };
        self.manager.load_timeline(&id).await?;
        let timeline_size = self
            .manager
            .timeline()
            .map(|timeline| timeline.read(&id))
            .transpose()?
            .map_or(0, |(_, rows)| rows.len());
        Ok(
            json!({"status":"agent_resumed","agentId":id,"agent":self.snapshot(&id)?,
            "timelineSize":timeline_size}),
        )
    }

    async fn resume_unknown(&mut self, mut request: ResumeRequest) -> Result<String, ErrorCode> {
        let metadata: crate::protocol::resume::Overrides =
            decode(json!(request.handle.metadata.clone().unwrap_or_default()))?;
        if request
            .overrides
            .provider
            .as_ref()
            .is_some_and(|provider| provider != &request.handle.provider)
        {
            return Err(ErrorCode::InvalidMessage);
        }
        let cwd = request
            .overrides
            .cwd
            .as_deref()
            .or(metadata.cwd.as_deref())
            .ok_or(ErrorCode::InvalidMessage)?;
        let canonical_cwd = super::placement::directory(cwd)?;
        let lookup_cwd =
            super::native_sessions::inspection_cwd(&request.handle.provider, cwd, &canonical_cwd);
        let history = self
            .manager
            .inspect_native(&request.handle, lookup_cwd)
            .await?;
        // Inspection supplies the verified filename and provider-specific lookup identity.
        request
            .handle
            .metadata
            .get_or_insert_with(Default::default)
            .extend(history.resume_metadata.clone());
        let mut record = super::native_sessions::new_record(&history, request.handle);
        metadata.apply(&mut record);
        request.overrides.apply(&mut record);
        record.cwd = canonical_cwd;
        let workspace = if let Some(directory) = &self.import_directory {
            Some(
                directory
                    .open_workspace(&record.cwd, &chrono::Utc::now().to_rfc3339())
                    .map_err(|_| ErrorCode::RegistryIo)?
                    .workspace_id,
            )
        } else {
            None
        };
        record.workspace_id = Some(self.workspace(workspace.as_deref(), &record.cwd)?);
        let restored = self
            .manager
            .restore_new(record)
            .await
            .map_err(|error| map_manager(&error))?;
        Ok(restored.id)
    }
}
