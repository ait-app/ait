use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use domain::bot::{Bot, BotState, Catalog, Channel, ChannelConfig, ExecutionConfig, PauseReason};
use domain::{BotId, ChannelId};
use model::ErrorCode;
use serde_json::{Value, json};

use super::State;
use crate::protocol::{
    ChannelCreate, ChannelRequest, ChannelUpdate, Setup, Update, WorkspaceRequest,
};

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
        validate_channel(&channel.config)?;
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

fn validate_channel(config: &ChannelConfig) -> Result<(), ErrorCode> {
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
    if !crate::ports::valid_credential_reference(reference)
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
        if let Ok(bot) = self.current(&request.workspace_id) {
            if bot.execution == request.execution {
                return Ok(json!({"bot":super::view(bot),"created":false}));
            }
            return Err(ErrorCode::BotConflict);
        }
        validate_execution(&request.execution)?;
        let bot = Bot {
            id: BotId::generate(),
            workspace_id: request.workspace_id,
            execution: request.execution,
            builtin_channel_id: ChannelId::generate(),
            state: BotState::Active,
            created_at: chrono::Utc::now().to_rfc3339(),
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
        bot.execution = request.execution;
        let view = super::view(bot);
        self.save_catalog(catalog)?;
        Ok(json!({"bot":view,"configurationAppliesToNewSessions":true}))
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
        self.retire_bots(&BTreeSet::from([id]))?;
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
    /// Retire `bots`, release their external channel accounts, and close their native
    /// sessions. Builtin channels stay so retired history keeps its routing.
    pub(super) fn retire_bots(&mut self, bots: &BTreeSet<BotId>) -> Result<(), ErrorCode> {
        let at = chrono::Utc::now().to_rfc3339();
        let mut catalog = self.catalog.clone();
        for bot in catalog.bots.iter_mut().filter(|bot| bots.contains(&bot.id)) {
            bot.state = BotState::Retired { at: at.clone() };
        }
        for channel in catalog.channels.iter_mut().filter(|channel| {
            bots.contains(&channel.bot_id) && !matches!(channel.config, ChannelConfig::Builtin)
        }) {
            channel.enabled = false;
            channel.deleted = true;
        }
        self.save_catalog(catalog)?;
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
        if self
            .catalog
            .channels
            .iter()
            .filter(|channel| channel.bot_id == bot && !channel.deleted)
            .count()
            >= 16
        {
            return Err(ErrorCode::ResourceExhausted);
        }
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
        Ok(json!({"deleted":true,"channelId":request.channel_id}))
    }
}
