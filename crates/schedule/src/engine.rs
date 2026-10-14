use chrono::{DateTime, Utc};
use domain::schedule::{Cadence, Run, RunStatus, Schedule, Status, Target};
use domain::storage::schedule::Error;
use model::storage::schedule::Store;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::cadence;
use crate::ports::Outcome;
use crate::protocol::Create;

pub(crate) struct Engine {
    store: Box<dyn Store>,
    records: Vec<Schedule>,
}
impl Engine {
    pub(crate) fn open(store: Box<dyn Store>, now: DateTime<Utc>) -> Result<Self, Error> {
        let mut records = store.load()?;
        if records.len() > 1024 {
            return Err(Error::Storage);
        }
        let mut ids = std::collections::BTreeSet::new();
        let mut dirty = false;
        for schedule in &mut records {
            if !ids.insert(schedule.id.clone())
                || Uuid::parse_str(&schedule.id).is_err()
                || schedule.runs.len() > 4096
            {
                return Err(Error::Storage);
            }
            validate(&schedule.prompt, &schedule.target, schedule.max_runs)?;
            if let Some(id) = &schedule.workspace_id {
                validate_workspace_id(id)?;
            }
            cadence::next(&schedule.cadence, now)?;
            for run in &mut schedule.runs {
                if run.status == RunStatus::Running {
                    run.status = RunStatus::Failed;
                    run.ended_at = Some(now);
                    run.error = Some("Server restarted before the scheduled run completed".into());
                    schedule.updated_at = now;
                    dirty = true;
                }
            }
            if schedule.status != Status::Completed && complete_due(schedule, now) {
                complete(schedule, now);
                dirty = true;
            }
            if schedule.status == Status::Active
                && schedule.next_run_at.is_some_and(|at| at <= now)
                && !matches!(schedule.cadence, Cadence::Once { .. })
            {
                schedule.next_run_at = Some(advance(
                    &schedule.cadence,
                    schedule.next_run_at.unwrap_or(now),
                    now,
                )?);
                schedule.updated_at = now;
                dirty = true;
            }
        }
        let mut result = Self { store, records };
        if dirty {
            result.store.save(&result.records)?;
        }
        Ok(result)
    }
    fn commit(&mut self, records: Vec<Schedule>) -> Result<(), Error> {
        self.store.save(&records)?;
        self.records = records;
        Ok(())
    }
    pub(crate) fn inspect(&self, id: &str) -> Result<Schedule, Error> {
        self.records
            .iter()
            .find(|s| s.id == id)
            .cloned()
            .ok_or(Error::NotFound)
    }
    pub(crate) fn request(
        &mut self,
        method: &str,
        params: Value,
        now: DateTime<Utc>,
    ) -> Result<Value, Error> {
        if method == "schedule.create.request" {
            return self.create(params, now);
        }
        if method == "schedule.list.request" {
            only(&params, &["workspaceId"])?;
            let workspace_id = params
                .get("workspaceId")
                .map(|value| value.as_str().ok_or(Error::Invalid))
                .transpose()?;
            let mut records = self.records.clone();
            if let Some(id) = workspace_id {
                validate_workspace_id(id)?;
                records.retain(|record| record.workspace_id.as_deref() == Some(id));
            }
            records.sort_by_key(|s| std::cmp::Reverse(s.created_at));
            return Ok(
                json!({"schedules":records.iter().map(summary).collect::<Result<Vec<_>,_>>()?,"error":null}),
            );
        }
        if method == "schedule.update.request" {
            return self.update(&params, now);
        }
        only(&params, &["scheduleId"])?;
        let id = params["scheduleId"].as_str().ok_or(Error::Invalid)?;
        if method == "schedule.delete.request" {
            if self.records.iter().any(|record| {
                record.id == id
                    && record
                        .runs
                        .iter()
                        .any(|run| run.status == RunStatus::Running)
            }) {
                return Err(Error::Conflict);
            }
            let mut records = self.records.clone();
            records.retain(|s| s.id != id);
            self.commit(records)?;
            return Ok(json!({"scheduleId":id,"error":null}));
        }
        let mut record = self.inspect(id)?;
        match method {
            "schedule.inspect.request" => return Ok(json!({"schedule":record,"error":null})),
            "schedule.logs.request" => return Ok(json!({"runs":record.runs,"error":null})),
            "schedule.pause.request" | "schedule.resume.request" => {
                if record.status == Status::Completed {
                    return Err(Error::Conflict);
                }
                let desired = if method == "schedule.pause.request" {
                    Status::Paused
                } else {
                    Status::Active
                };
                if record.status != desired {
                    record.status = desired;
                    record.updated_at = now;
                    record.paused_at = if desired == Status::Paused {
                        Some(now)
                    } else {
                        None
                    };
                    record.next_run_at = if desired == Status::Paused {
                        None
                    } else {
                        Some(first_trigger(&record.cadence, record.start_at, now)?.max(now))
                    };
                    self.replace(record.clone())?;
                }
            }
            _ => return Err(Error::Invalid),
        }
        Ok(json!({"schedule":summary(&record)?,"error":null}))
    }
    fn create(&mut self, params: Value, now: DateTime<Utc>) -> Result<Value, Error> {
        let request: Create = serde_json::from_value(params).map_err(|_| Error::Invalid)?;
        validate(&request.prompt, &request.target, request.max_runs)?;
        validate_workspace_id(&request.workspace_id)?;
        validate_trigger(&request.cadence, request.start_at, now)?;
        if request.start_at.is_some_and(|at| at < now) {
            return Err(Error::Invalid);
        }
        let next = first_trigger(&request.cadence, request.start_at, now)?;
        let immediate = request.run_on_create.unwrap_or(
            request.start_at.is_none() && matches!(request.cadence, Cadence::Every { .. }),
        );
        let next = if immediate { now } else { next };
        if request.expires_at.is_some_and(|at| at <= now || at <= next)
            || (request.start_at.is_some() && request.run_on_create == Some(true))
            || (matches!(request.cadence, Cadence::Once { .. })
                && request.run_on_create == Some(true))
        {
            return Err(Error::Invalid);
        }
        if self.records.len() >= 1024 {
            return Err(Error::Conflict);
        }
        let record = Schedule {
            id: Uuid::new_v4().to_string(),
            workspace_id: Some(request.workspace_id),
            name: normalize_name(request.name),
            prompt: request.prompt.trim().to_owned(),
            cadence: request.cadence,
            start_at: request.start_at,
            target: request.target,
            status: Status::Active,
            created_at: now,
            updated_at: now,
            next_run_at: Some(next),
            last_run_at: None,
            paused_at: None,
            expires_at: request.expires_at,
            max_runs: request.max_runs,
            runs: vec![],
        };
        let value = summary(&record)?;
        let mut records = self.records.clone();
        records.push(record);
        self.commit(records)?;
        Ok(json!({"schedule":value,"error":null}))
    }
    fn replace(&mut self, record: Schedule) -> Result<(), Error> {
        let mut records = self.records.clone();
        let index = records
            .iter()
            .position(|s| s.id == record.id)
            .ok_or(Error::NotFound)?;
        records[index] = record;
        self.commit(records)
    }
    fn update(&mut self, params: &Value, now: DateTime<Utc>) -> Result<Value, Error> {
        let record = self.preview_update(params, now)?;
        self.replace(record.clone())?;
        Ok(json!({"schedule":record,"error":null}))
    }
    pub(crate) fn preview_update(
        &self,
        params: &Value,
        now: DateTime<Utc>,
    ) -> Result<Schedule, Error> {
        only(
            params,
            &[
                "scheduleId",
                "name",
                "prompt",
                "cadence",
                "newAgentConfig",
                "maxRuns",
                "expiresAt",
                "workspaceId",
                "target",
                "startAt",
            ],
        )?;
        let mut record = self.inspect(params["scheduleId"].as_str().ok_or(Error::Invalid)?)?;
        if ["target", "newAgentConfig", "workspaceId"]
            .iter()
            .any(|field| params.get(field).is_some())
            && record
                .runs
                .iter()
                .any(|run| run.status == RunStatus::Running)
        {
            return Err(Error::Conflict);
        }
        if let Some(value) = params.get("workspaceId") {
            let id = value.as_str().ok_or(Error::Invalid)?;
            validate_workspace_id(id)?;
            if record
                .workspace_id
                .as_deref()
                .is_some_and(|current| current != id)
            {
                return Err(Error::Invalid);
            }
            record.workspace_id = Some(id.to_owned());
        }
        if let Some(value) = params.get("target") {
            if params.get("newAgentConfig").is_some() {
                return Err(Error::Invalid);
            }
            record.target = serde_json::from_value(value.clone()).map_err(|_| Error::Invalid)?;
        }
        if let Some(value) = params.get("startAt") {
            record.start_at = serde_json::from_value(value.clone()).map_err(|_| Error::Invalid)?;
            if record.start_at.is_some_and(|at| at < now) {
                return Err(Error::Invalid);
            }
        }
        if let Some(value) = params.get("name") {
            record.name =
                normalize_name(serde_json::from_value(value.clone()).map_err(|_| Error::Invalid)?);
        }
        if let Some(value) = params.get("prompt") {
            value
                .as_str()
                .ok_or(Error::Invalid)?
                .trim()
                .clone_into(&mut record.prompt);
        }
        if let Some(value) = params.get("cadence") {
            let mut next: Cadence =
                serde_json::from_value(value.clone()).map_err(|_| Error::Invalid)?;
            if let (Cadence::Cron { timezone: old, .. }, Cadence::Cron { timezone: next, .. }) =
                (&record.cadence, &mut next)
                && next.is_none()
            {
                next.clone_from(old);
            }
            validate_trigger(&next, record.start_at, now)?;
            let at = first_trigger(&next, record.start_at, now)?;
            record.cadence = next;
            record.next_run_at = (record.status == Status::Active).then_some(at);
        }
        if params.get("startAt").is_some() && params.get("cadence").is_none() {
            validate_trigger(&record.cadence, record.start_at, now)?;
            record.next_run_at = (record.status == Status::Active).then_some(first_trigger(
                &record.cadence,
                record.start_at,
                now,
            )?);
        }
        if let Some(value) = params.get("newAgentConfig") {
            patch_new_agent_config(&mut record.target, value)?;
        }
        if let Some(value) = params.get("maxRuns") {
            record.max_runs = serde_json::from_value(value.clone()).map_err(|_| Error::Invalid)?;
        }
        if let Some(value) = params.get("expiresAt") {
            record.expires_at =
                serde_json::from_value(value.clone()).map_err(|_| Error::Invalid)?;
        }
        validate(&record.prompt, &record.target, record.max_runs)?;
        record.updated_at = now;
        Ok(record)
    }
    pub(crate) fn due(&mut self, now: DateTime<Utc>) -> Result<Vec<String>, Error> {
        let schedulable = |schedule: &Schedule| {
            schedule.status == Status::Active
                && schedule.next_run_at.is_some()
                && !schedule.runs.iter().any(|r| r.status == RunStatus::Running)
        };
        let mut due = Vec::new();
        let mut expired = false;
        for schedule in self.records.iter().filter(|schedule| schedulable(schedule)) {
            if complete_due(schedule, now) {
                expired = true;
            } else if schedule.next_run_at.is_some_and(|at| at <= now) {
                due.push(schedule.id.clone());
            }
        }
        // Copy the records only when one must be committed as completed.
        if expired {
            let mut records = self.records.clone();
            for schedule in &mut records {
                if schedulable(schedule) && complete_due(schedule, now) {
                    complete(schedule, now);
                }
            }
            self.commit(records)?;
        }
        Ok(due)
    }
    pub(crate) fn begin(
        &mut self,
        id: &str,
        manual: bool,
        now: DateTime<Utc>,
    ) -> Result<(Schedule, String), Error> {
        let mut schedule = self.inspect(id)?;
        if complete_due(&schedule, now) && schedule.status != Status::Completed {
            complete(&mut schedule, now);
            self.replace(schedule)?;
            return Err(Error::Conflict);
        }
        if schedule.status == Status::Completed
            || (!manual && schedule.status != Status::Active)
            || schedule.runs.len() >= 4096
            || schedule.runs.iter().any(|r| r.status == RunStatus::Running)
        {
            return Err(Error::Conflict);
        }
        let run_id = Uuid::new_v4().to_string();
        schedule.runs.push(Run {
            id: run_id.clone(),
            scheduled_for: if manual {
                now
            } else {
                schedule.next_run_at.unwrap_or(now)
            },
            started_at: now,
            ended_at: None,
            status: RunStatus::Running,
            agent_id: None,
            workspace_id: schedule.workspace_id.clone(),
            output: None,
            error: None,
        });
        schedule.updated_at = now;
        self.replace(schedule.clone())?;
        Ok((schedule, run_id))
    }
    pub(crate) fn checkpoint(&mut self, update: &crate::ports::Checkpoint) -> Result<(), Error> {
        let mut schedule = self.inspect(&update.schedule_id)?;
        let run = schedule
            .runs
            .iter_mut()
            .find(|r| r.id == update.run_id && r.status == RunStatus::Running)
            .ok_or(Error::NotFound)?;
        run.agent_id.clone_from(&update.agent_id);
        run.workspace_id.clone_from(&update.workspace_id);
        self.replace(schedule)
    }
    pub(crate) fn finish(
        &mut self,
        id: &str,
        run_id: &str,
        manual: bool,
        outcome: Outcome,
        now: DateTime<Utc>,
    ) -> Result<(), Error> {
        let mut schedule = match self.inspect(id) {
            Ok(s) => s,
            Err(Error::NotFound) => return Ok(()),
            Err(e) => return Err(e),
        };
        let run = schedule
            .runs
            .iter_mut()
            .find(|r| r.id == run_id)
            .ok_or(Error::NotFound)?;
        run.status = if outcome.error.is_some() {
            RunStatus::Failed
        } else {
            RunStatus::Succeeded
        };
        run.ended_at = Some(now);
        if outcome.agent_id.is_some() {
            run.agent_id = outcome.agent_id;
        }
        if outcome.workspace_id.is_some() {
            run.workspace_id = outcome.workspace_id;
        }
        run.output = outcome.output.map(|s| limit_text(&s));
        run.error = outcome.error.map(|s| limit_text(&s));
        schedule.last_run_at = Some(now);
        schedule.updated_at = now;
        if outcome.target_gone || schedule.runs.len() >= 4096 || complete_due(&schedule, now) {
            complete(&mut schedule, now);
        } else if !manual && schedule.status == Status::Active {
            schedule.next_run_at = Some(advance(
                &schedule.cadence,
                schedule.next_run_at.unwrap_or(now),
                now,
            )?);
        }
        self.replace(schedule)
    }
}
fn advance(
    cadence: &Cadence,
    anchor: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<DateTime<Utc>, Error> {
    // An in-flight run can finish after an edit/resume has already scheduled a future slot.
    if anchor > now {
        return Ok(anchor);
    }
    if let Cadence::Every { every_ms } = cadence {
        let delta = now.signed_duration_since(anchor).num_milliseconds().max(0);
        let steps = delta
            .checked_div(*every_ms)
            .and_then(|n| n.checked_add(1))
            .ok_or(Error::Invalid)?;
        return anchor
            .checked_add_signed(chrono::Duration::milliseconds(
                steps.checked_mul(*every_ms).ok_or(Error::Invalid)?,
            ))
            .ok_or(Error::Invalid);
    }
    cadence::next(cadence, now)
}
fn summary(schedule: &Schedule) -> Result<Value, Error> {
    let mut value = serde_json::to_value(schedule).map_err(|_| Error::Storage)?;
    value.as_object_mut().ok_or(Error::Storage)?.remove("runs");
    Ok(value)
}
fn normalize_name(name: Option<String>) -> Option<String> {
    name.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty())
}
fn complete_due(schedule: &Schedule, now: DateTime<Utc>) -> bool {
    schedule.expires_at.is_some_and(|at| at <= now)
        || (matches!(schedule.cadence, Cadence::Once { .. })
            && schedule
                .runs
                .iter()
                .any(|run| run.status != RunStatus::Running))
        || schedule.max_runs.is_some_and(|max| {
            schedule
                .runs
                .iter()
                .filter(|r| r.status != RunStatus::Running)
                .count() as u64
                >= max
        })
}
fn complete(schedule: &mut Schedule, now: DateTime<Utc>) {
    schedule.status = Status::Completed;
    schedule.next_run_at = None;
    schedule.paused_at = None;
    schedule.updated_at = now;
}
pub(crate) fn only(value: &Value, fields: &[&str]) -> Result<(), Error> {
    if !value
        .as_object()
        .is_some_and(|map| map.keys().all(|key| fields.contains(&key.as_str())))
    {
        return Err(Error::Invalid);
    }
    Ok(())
}
fn validate(prompt: &str, target: &Target, max_runs: Option<u64>) -> Result<(), Error> {
    if prompt.trim().is_empty() || prompt.len() > 64 * 1024 || max_runs == Some(0) {
        return Err(Error::Invalid);
    }
    match target {
        Target::Agent { agent_id } => {
            Uuid::parse_str(agent_id).map_err(|_| Error::Invalid)?;
        }
        Target::NewAgent { config } => {
            only(
                config,
                &[
                    "provider",
                    "cwd",
                    "modeId",
                    "model",
                    "thinkingOptionId",
                    "archiveOnFinish",
                    "isolation",
                    "title",
                    "providerOptions",
                    "featureValues",
                    "systemPrompt",
                    "mcpServers",
                ],
            )?;
            for key in ["provider"] {
                if config[key].as_str().is_none_or(|s| s.trim().is_empty()) {
                    return Err(Error::Invalid);
                }
            }
            for key in ["cwd", "model", "modeId", "thinkingOptionId", "isolation"] {
                if let Some(value) = config.get(key)
                    && value.as_str().is_none_or(|s| s.trim().is_empty())
                {
                    return Err(Error::Invalid);
                }
            }
            if config
                .get("isolation")
                .is_some_and(|v| v != "local" && v != "worktree")
                || config
                    .get("archiveOnFinish")
                    .is_some_and(|v| !v.is_boolean())
            {
                return Err(Error::Invalid);
            }
        }
    }
    Ok(())
}
fn patch_new_agent_config(target: &mut Target, value: &Value) -> Result<(), Error> {
    only(
        value,
        &[
            "provider",
            "cwd",
            "model",
            "modeId",
            "thinkingOptionId",
            "archiveOnFinish",
            "isolation",
        ],
    )?;
    let Target::NewAgent { config } = target else {
        return Err(Error::Invalid);
    };
    for (key, value) in value.as_object().ok_or(Error::Invalid)? {
        if ["model", "modeId", "thinkingOptionId"].contains(&key.as_str())
            && (value.is_null() || value.as_str().is_some_and(|s| s.trim().is_empty()))
        {
            config.as_object_mut().ok_or(Error::Invalid)?.remove(key);
        } else {
            config[key] = value.clone();
        }
    }
    Ok(())
}

fn validate_workspace_id(id: &str) -> Result<(), Error> {
    if id.trim().is_empty() || id.trim() != id || id.len() > 256 {
        return Err(Error::Invalid);
    }
    Ok(())
}
fn validate_trigger(
    cadence: &Cadence,
    start_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Result<(), Error> {
    if let Cadence::Once { at } = cadence
        && (*at <= now || start_at.is_some())
    {
        return Err(Error::Invalid);
    }
    cadence::next(cadence, now).map(|_| ())
}
fn first_trigger(
    cadence: &Cadence,
    start_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Result<DateTime<Utc>, Error> {
    match cadence {
        Cadence::Every { .. } if start_at.is_some_and(|at| at >= now) => {
            start_at.ok_or(Error::Invalid)
        }
        Cadence::Cron { .. } => cadence::next(
            cadence,
            start_at.map_or(now, |at| {
                at.checked_sub_signed(chrono::Duration::milliseconds(1))
                    .unwrap_or(at)
                    .max(now)
            }),
        ),
        _ => cadence::next(cadence, now),
    }
}
fn limit_text(text: &str) -> String {
    let end = text.floor_char_boundary((64 * 1024).min(text.len()));
    text[..end].to_owned()
}
#[cfg(test)]
pub(crate) mod tests;

#[cfg(test)]
mod storage_tests;
