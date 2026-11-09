use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use domain::bot::{Bot, BotState, Catalog, Channel, ChannelConfig, ExecutionConfig, PauseReason};
use domain::{BotId, ChannelId};
use model::ErrorCode;
use secrecy::SecretString;
use serde_json::{Value, json};

use super::{State, StoredChannel};
use crate::ports::Credentials;
use crate::protocol::{
    ChannelCreate, ChannelRequest, ChannelUpdate, PermissionsUpdate, Setup, Update,
    WorkspaceRequest,
};

/// Most undeleted Channels one Bot may own.
const MAX_CHANNELS: usize = 16;
/// Most Bots that are not retired.
const LIVE_BOTS: usize = 128;
/// Most undeleted Channels owned by Bots that are not retired.
const LIVE_CHANNELS: usize = 256;
/// Hard cap on stored Bots; retired Bots keep their history, so it exceeds the live limit.
const STORED_BOTS: usize = 4096;
/// Hard cap on stored Channels; deleted Channels keep their history, so it exceeds the live limit.
const STORED_CHANNELS: usize = 8192;

fn validate_capacity(catalog: &Catalog) -> Result<(), ErrorCode> {
    if catalog.bots.len() > STORED_BOTS || catalog.channels.len() > STORED_CHANNELS {
        return Err(ErrorCode::ResourceExhausted);
    }
    let retired_bots: BTreeSet<_> = catalog
        .bots
        .iter()
        .filter(|bot| retired(bot))
        .map(|bot| bot.id)
        .collect();
    let live_channels = catalog
        .channels
        .iter()
        .filter(|channel| !channel.deleted && !retired_bots.contains(&channel.bot_id))
        .count();
    let live_bots = catalog.bots.len() - retired_bots.len();
    if live_bots > LIVE_BOTS || live_channels > LIVE_CHANNELS {
        return Err(ErrorCode::ResourceExhausted);
    }
    Ok(())
}

pub(super) fn validate(catalog: &Catalog) -> Result<(), ErrorCode> {
    validate_capacity(catalog)?;
    let mut bots = BTreeMap::new();
    let mut workspaces = BTreeSet::new();
    for bot in &catalog.bots {
        validate_execution(&bot.execution)?;
        bot.permissions
            .clone()
            .normalized()
            .map_err(|_| ErrorCode::InvalidMessage)?;
        if !model::valid_id(&bot.workspace_id)
            || bots.insert(bot.id, bot).is_some()
            || (!retired(bot) && !workspaces.insert(bot.workspace_id.as_str()))
        {
            return Err(ErrorCode::InvalidMessage);
        }
    }
    let mut channels = BTreeSet::new();
    let mut accounts = BTreeSet::new();
    let mut builtin = 0;
    for channel in &catalog.channels {
        let bot = bots.get(&channel.bot_id).ok_or(ErrorCode::InvalidMessage)?;
        validate_channel(channel.id, &channel.config)?;
        if !channels.insert(channel.id) {
            return Err(ErrorCode::InvalidMessage);
        }
        if matches!(channel.config, ChannelConfig::Builtin) {
            if channel.id != bot.builtin_channel_id || channel.deleted {
                return Err(ErrorCode::InvalidMessage);
            }
            builtin += 1;
        }
        if !channel.deleted
            && let Some(account) = account(&channel.config)
            && !accounts.insert(account)
        {
            return Err(ErrorCode::BotConflict);
        }
    }
    // Builtin channel IDs are unique and equal their Bot's, so equal counts mean one per Bot.
    if builtin != bots.len() {
        return Err(ErrorCode::InvalidMessage);
    }
    Ok(())
}

fn retired(bot: &Bot) -> bool {
    matches!(bot.state, BotState::Retired { .. })
}

fn validate_execution(execution: &ExecutionConfig) -> Result<(), ErrorCode> {
    if !model::valid_id(&execution.provider)
        || [&execution.model, &execution.mode_id, &execution.thinking]
            .into_iter()
            .flatten()
            .any(|value| !model::valid_id(value))
    {
        return Err(ErrorCode::InvalidMessage);
    }
    Ok(())
}

/// Validate `execution` and drop the mode: a Bot's native mode is chosen by the server.
fn normalized_execution(mut execution: ExecutionConfig) -> Result<ExecutionConfig, ErrorCode> {
    validate_execution(&execution)?;
    execution.mode_id = None;
    Ok(execution)
}

fn validate_channel(id: ChannelId, config: &ChannelConfig) -> Result<(), ErrorCode> {
    let (reference, senders) = match config {
        ChannelConfig::Builtin => return Ok(()),
        ChannelConfig::Telegram {
            token_ref,
            allowed_senders,
            ..
        } => (token_ref, allowed_senders),
        ChannelConfig::Feishu {
            app_id,
            secret_ref,
            allowed_senders,
            ..
        } => {
            if !model::valid_id(app_id) {
                return Err(ErrorCode::InvalidMessage);
            }
            (secret_ref, allowed_senders)
        }
    };
    // A stored reference names only its own channel, so no client can borrow another's secret.
    let reference_allowed = match crate::ports::stored_credential_channel(reference) {
        Some(owner) => owner == id,
        None => crate::ports::valid_credential_reference(reference),
    };
    if !reference_allowed
        || senders.is_empty()
        || senders.len() > 256
        || senders.iter().any(|sender| !model::valid_id(sender))
    {
        return Err(ErrorCode::InvalidMessage);
    }
    Ok(())
}

fn account(config: &ChannelConfig) -> Option<(&'static str, &str)> {
    match config {
        ChannelConfig::Builtin => None,
        ChannelConfig::Telegram { token_ref, .. } => Some(("telegram", token_ref)),
        ChannelConfig::Feishu { app_id, .. } => Some(("feishu", app_id)),
    }
}

impl State {
    pub(super) fn setup(&mut self, request: Setup) -> Result<Value, ErrorCode> {
        self.active_workspace(&request.workspace_id)?;
        let execution = normalized_execution(request.execution)?;
        let permissions = request
            .permissions
            .unwrap_or_default()
            .normalized()
            .map_err(|_| ErrorCode::InvalidMessage)?;
        if let Ok(bot) = self.current(&request.workspace_id) {
            if bot.execution == execution && bot.permissions == permissions {
                return Ok(json!({"bot":super::view(bot),"created":false}));
            }
            return Err(ErrorCode::BotConflict);
        }
        let bot = Bot {
            id: BotId::generate(),
            workspace_id: request.workspace_id,
            execution,
            builtin_channel_id: ChannelId::generate(),
            state: BotState::Active,
            created_at: chrono::Utc::now().to_rfc3339(),
            permissions,
            workspaces: BTreeSet::new(),
        };
        let mut catalog = self.catalog.clone();
        catalog.channels.push(Channel {
            id: bot.builtin_channel_id,
            bot_id: bot.id,
            config: ChannelConfig::Builtin,
            enabled: true,
            cursor: None,
            deleted: false,
        });
        let view = super::view(&bot);
        let workspace_id = bot.workspace_id.clone();
        catalog.bots.push(bot);
        self.save_catalog(catalog)?;
        // Memory is cleared only once the new Bot is durable, so a refused setup keeps it.
        if request.clear_memory && self.store.delete_memory(&workspace_id).is_err() {
            self.frozen = true;
            return Err(ErrorCode::BotIo);
        }
        Ok(json!({"bot":view,"created":true}))
    }
    pub(super) fn update(&mut self, request: Update) -> Result<Value, ErrorCode> {
        let id = self.current(&request.workspace_id)?.id;
        self.active_workspace(&request.workspace_id)?;
        let mut catalog = self.catalog.clone();
        let bot = catalog
            .bots
            .iter_mut()
            .find(|entry| entry.id == id)
            .ok_or(ErrorCode::BotNotFound)?;
        bot.execution = normalized_execution(request.execution)?;
        let view = super::view(bot);
        self.save_catalog(catalog)?;
        Ok(json!({"bot":view,"configurationAppliesToNewSessions":true}))
    }
    /// Replace the current Bot's tool groups and mode caps; the next turn reads them.
    pub(super) fn update_permissions(
        &mut self,
        request: PermissionsUpdate,
    ) -> Result<Value, ErrorCode> {
        let id = self.current(&request.workspace_id)?.id;
        let permissions = request
            .permissions
            .normalized()
            .map_err(|_| ErrorCode::InvalidMessage)?;
        let mut catalog = self.catalog.clone();
        let bot = catalog
            .bots
            .iter_mut()
            .find(|entry| entry.id == id)
            .ok_or(ErrorCode::BotNotFound)?;
        bot.permissions = permissions;
        let view = super::view(bot);
        self.save_catalog(catalog)?;
        Ok(json!({"bot":view,"appliesToNextTurn":true}))
    }
    /// Pause the Workspace's active Bot at the user's request.
    pub(super) fn pause(&mut self, request: &WorkspaceRequest) -> Result<Value, ErrorCode> {
        let bot = self.current(&request.workspace_id)?;
        if !matches!(bot.state, BotState::Active) {
            return Err(ErrorCode::BotConflict);
        }
        let view = self.pause_bot(bot.id, PauseReason::User)?;
        Ok(json!({"bot":view}))
    }
    /// Reactivate a paused Bot once its Workspace is active and its directory exists.
    pub(super) fn resume(&mut self, request: &WorkspaceRequest) -> Result<Value, ErrorCode> {
        let bot = self.current(&request.workspace_id)?;
        if !matches!(bot.state, BotState::Paused { .. }) {
            return Err(ErrorCode::BotConflict);
        }
        let id = bot.id;
        let cwd = self.active_workspace(&request.workspace_id)?;
        if !Path::new(&cwd).is_dir() {
            return Err(ErrorCode::WorkspaceNotFound);
        }
        let view = self.set_state(id, BotState::Active)?;
        Ok(json!({"bot":view}))
    }
    /// Retire the Workspace's Bot at the user's request; its memory is kept for a later setup.
    pub(super) fn remove(&mut self, request: &WorkspaceRequest) -> Result<Value, ErrorCode> {
        let id = self.current(&request.workspace_id)?.id;
        self.retire_bots(self.catalog.clone(), &BTreeSet::from([id]))?;
        Ok(json!({"bot":super::view(self.bot(id)?)}))
    }
    /// Persist `reason` as the Bot's pause cause, then cancel its queued work and release
    /// its native sessions.
    pub(super) fn pause_bot(&mut self, id: BotId, reason: PauseReason) -> Result<Value, ErrorCode> {
        let cancellation = match reason {
            PauseReason::DirectoryMissing => Some("Workspace directory missing; Bot paused"),
            PauseReason::User => None,
        };
        let view = self.set_state(id, BotState::Paused { reason })?;
        self.stop_bot(id, false, cancellation)?;
        Ok(view)
    }
    /// Save `catalog` with `bots` retired and their external channel accounts released, then
    /// close their native sessions. Builtin channels stay so retired history keeps its routing.
    pub(super) fn retire_bots(
        &mut self,
        mut catalog: Catalog,
        bots: &BTreeSet<BotId>,
    ) -> Result<(), ErrorCode> {
        let at = chrono::Utc::now().to_rfc3339();
        for bot in catalog.bots.iter_mut().filter(|bot| bots.contains(&bot.id)) {
            bot.state = BotState::Retired { at: at.clone() };
        }
        let mut released = Vec::new();
        for channel in catalog.channels.iter_mut().filter(|channel| {
            bots.contains(&channel.bot_id) && !matches!(channel.config, ChannelConfig::Builtin)
        }) {
            channel.enabled = false;
            channel.deleted = true;
            released.push(channel.id);
        }
        self.save_catalog(catalog)?;
        self.released_credentials.extend(released);
        for bot in bots {
            self.stop_bot(*bot, true, None)?;
        }
        Ok(())
    }
    fn set_state(&mut self, id: BotId, state: BotState) -> Result<Value, ErrorCode> {
        let mut catalog = self.catalog.clone();
        let bot = catalog
            .bots
            .iter_mut()
            .find(|entry| entry.id == id)
            .ok_or(ErrorCode::BotNotFound)?;
        bot.state = state;
        let view = super::view(bot);
        self.save_catalog(catalog)?;
        Ok(view)
    }
    pub(super) fn create_channel(&mut self, request: ChannelCreate) -> Result<Value, ErrorCode> {
        let bot = self.current(&request.workspace_id)?;
        self.workspace(bot)?;
        let bot = bot.id;
        if matches!(request.config, ChannelConfig::Builtin) {
            return Err(ErrorCode::BotConflict);
        }
        if let Some(channel) = self
            .catalog
            .channels
            .iter()
            .find(|channel| channel.id == request.channel_id)
        {
            if channel.bot_id == bot
                && channel.config == request.config
                && channel.enabled == request.enabled
                && !channel.deleted
            {
                return Ok(json!({"channel":super::project_channel(channel),"created":false}));
            }
            return Err(ErrorCode::IdempotencyConflict);
        }
        self.under_channel_limit(bot)?;
        let channel = Channel {
            id: request.channel_id,
            bot_id: bot,
            config: request.config,
            enabled: request.enabled,
            cursor: None,
            deleted: false,
        };
        let mut catalog = self.catalog.clone();
        catalog.channels.push(channel.clone());
        self.save_catalog(catalog)?;
        Ok(json!({"channel":super::project_channel(&channel),"created":true}))
    }
    fn under_channel_limit(&self, bot: BotId) -> Result<(), ErrorCode> {
        let channels = self
            .catalog
            .channels
            .iter()
            .filter(|channel| channel.bot_id == bot && !channel.deleted)
            .count();
        if channels >= MAX_CHANNELS {
            return Err(ErrorCode::ResourceExhausted);
        }
        Ok(())
    }
    /// Refuse before an external account is created for a channel the Bot could not keep.
    pub(crate) fn check_channel_admission(&self, workspace: &str) -> Result<(), ErrorCode> {
        let bot = self.current(workspace)?;
        self.workspace(bot)?;
        self.under_channel_limit(bot.id)
    }
    /// Store `secret`, then enable the channel that references it. A retry of an existing
    /// channel never rewrites its secret; a refused new channel leaves no secret behind.
    pub(crate) fn create_stored_channel(
        &mut self,
        channel: StoredChannel,
        secret: &SecretString,
        credentials: &dyn Credentials,
    ) -> Result<Value, ErrorCode> {
        let id = channel.channel_id;
        let request = ChannelCreate {
            workspace_id: channel.workspace_id,
            channel_id: id,
            config: channel.config,
            enabled: true,
        };
        if self.catalog.channels.iter().any(|entry| entry.id == id) {
            return self.create_channel(request);
        }
        credentials.store(id, secret)?;
        let created = self.create_channel(request);
        // A frozen state means the catalog write may have reached disk with this channel in it.
        if created.is_err()
            && !self.frozen
            && let Err(error) = credentials.delete(id)
        {
            tracing::warn!(channel = %id, ?error, "could not remove a refused channel's secret");
        }
        created
    }
    pub(super) fn update_channel(&mut self, request: ChannelUpdate) -> Result<Value, ErrorCode> {
        let bot = self.current(&request.workspace_id)?;
        self.workspace(bot)?;
        let mut channel = self.channel(bot.id, request.channel_id)?.clone();
        if matches!(channel.config, ChannelConfig::Builtin) || channel.deleted {
            return Err(ErrorCode::BotConflict);
        }
        if let Some(config) = request.config {
            if std::mem::discriminant(&config) != std::mem::discriminant(&channel.config)
                || account(&config) != account(&channel.config)
            {
                return Err(ErrorCode::BotConflict);
            }
            channel.config = config;
        }
        channel.enabled = request.enabled;
        let mut catalog = self.catalog.clone();
        *catalog
            .channels
            .iter_mut()
            .find(|entry| entry.id == channel.id)
            .ok_or(ErrorCode::BotNotFound)? = channel.clone();
        self.save_catalog(catalog)?;
        Ok(json!({"channel":super::project_channel(&channel)}))
    }
    pub(super) fn delete_channel(&mut self, request: &ChannelRequest) -> Result<Value, ErrorCode> {
        let bot = self.current(&request.workspace_id)?.id;
        let channel = self.channel(bot, request.channel_id)?;
        if matches!(channel.config, ChannelConfig::Builtin) {
            return Err(ErrorCode::BotConflict);
        }
        let mut catalog = self.catalog.clone();
        let channel = catalog
            .channels
            .iter_mut()
            .find(|entry| entry.id == request.channel_id)
            .ok_or(ErrorCode::BotNotFound)?;
        channel.enabled = false;
        channel.deleted = true;
        self.save_catalog(catalog)?;
        self.released_credentials.push(request.channel_id);
        Ok(json!({"deleted":true,"channelId":request.channel_id}))
    }
}
