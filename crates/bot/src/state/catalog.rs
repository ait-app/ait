use std::collections::{BTreeMap, BTreeSet};

use domain::ChannelId;
use domain::bot::{Bot, Catalog, Channel, ChannelConfig, ExecutionConfig};
use model::ErrorCode;
use serde_json::{Value, json};

use super::State;
use crate::protocol::{BotRequest, ChannelCreate, ChannelRequest, ChannelUpdate, Create, Update};

/// Most Bots that are not archived.
const LIVE_BOTS: usize = 128;
/// Most undeleted Channels owned by Bots that are not archived.
const LIVE_CHANNELS: usize = 256;
/// Hard cap on stored Bots; archived Bots keep their history, so it exceeds the live limit.
const STORED_BOTS: usize = 4096;
/// Hard cap on stored Channels; deleted Channels keep their history, so it exceeds the live limit.
const STORED_CHANNELS: usize = 8192;

fn validate_capacity(catalog: &Catalog) -> Result<(), ErrorCode> {
    if catalog.bots.len() > STORED_BOTS || catalog.channels.len() > STORED_CHANNELS {
        return Err(ErrorCode::ResourceExhausted);
    }
    let archived: BTreeSet<_> = catalog
        .bots
        .iter()
        .filter(|bot| bot.archived_at.is_some())
        .map(|bot| bot.id)
        .collect();
    let live_channels = catalog
        .channels
        .iter()
        .filter(|channel| !channel.deleted && !archived.contains(&channel.bot_id))
        .count();
    let live_bots = catalog
        .bots
        .iter()
        .filter(|bot| bot.archived_at.is_none())
        .count();
    if live_bots > LIVE_BOTS || live_channels > LIVE_CHANNELS {
        return Err(ErrorCode::ResourceExhausted);
    }
    Ok(())
}

pub(super) fn validate(catalog: &Catalog) -> Result<(), ErrorCode> {
    validate_capacity(catalog)?;
    let mut bots = BTreeMap::new();
    for bot in &catalog.bots {
        validate_bot(&bot.name, &bot.instructions, &bot.execution)?;
        if !model::valid_id(&bot.workspace_id) || bots.insert(bot.id, bot).is_some() {
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

fn validate_bot(
    name: &str,
    instructions: &str,
    execution: &ExecutionConfig,
) -> Result<(), ErrorCode> {
    if name.trim().is_empty()
        || name.len() > 256
        || name.chars().any(char::is_control)
        || instructions.len() > 16 * 1024
        || instructions.contains('\0')
        || !model::valid_id(&execution.provider)
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
    pub(super) fn create(&mut self, request: Create) -> Result<Value, ErrorCode> {
        if let Ok(bot) = self.bot(request.bot_id) {
            if bot.workspace_id == request.workspace_id
                && bot.name == request.name
                && bot.instructions == request.instructions
                && bot.execution == request.execution
            {
                return Ok(json!({"bot":bot,"created":false}));
            }
            return Err(ErrorCode::IdempotencyConflict);
        }
        self.active_workspace(&request.workspace_id)?;
        let mut catalog = self.catalog.clone();
        let bot = Bot {
            id: request.bot_id,
            workspace_id: request.workspace_id,
            name: request.name,
            instructions: request.instructions,
            execution: request.execution,
            builtin_channel_id: ChannelId::generate(),
            enabled: true,
            archived_at: None,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        catalog.channels.push(Channel {
            id: bot.builtin_channel_id,
            bot_id: bot.id,
            config: ChannelConfig::Builtin,
            enabled: true,
            cursor: None,
            deleted: false,
        });
        catalog.bots.push(bot.clone());
        self.save_catalog(catalog)?;
        Ok(json!({"bot":bot,"created":true}))
    }
    pub(super) fn update(&mut self, request: Update) -> Result<Value, ErrorCode> {
        let mut bot = self.bot(request.bot_id)?.clone();
        if bot.archived_at.is_some() {
            return Err(ErrorCode::BotConflict);
        }
        self.active_workspace(&bot.workspace_id)?;
        if let Some(name) = request.name {
            bot.name = name;
        }
        if let Some(instructions) = request.instructions {
            bot.instructions = instructions;
        }
        if let Some(execution) = request.execution {
            bot.execution = execution;
        }
        if let Some(enabled) = request.enabled {
            bot.enabled = enabled;
        }
        let mut catalog = self.catalog.clone();
        *catalog
            .bots
            .iter_mut()
            .find(|entry| entry.id == bot.id)
            .ok_or(ErrorCode::BotNotFound)? = bot.clone();
        self.save_catalog(catalog)?;
        if !bot.enabled {
            self.stop_bot(bot.id, false)?;
        }
        Ok(json!({"bot":bot,"configurationAppliesToNewSessions":true}))
    }
    pub(super) fn archive(&mut self, request: BotRequest) -> Result<Value, ErrorCode> {
        self.bot(request.bot_id)?;
        let mut catalog = self.catalog.clone();
        let bot = catalog
            .bots
            .iter_mut()
            .find(|bot| bot.id == request.bot_id)
            .ok_or(ErrorCode::BotNotFound)?;
        bot.enabled = false;
        bot.archived_at
            .get_or_insert_with(|| chrono::Utc::now().to_rfc3339());
        self.save_catalog(catalog)?;
        self.stop_bot(request.bot_id, true)?;
        Ok(json!({"bot":self.bot(request.bot_id)?}))
    }
    pub(super) fn create_channel(&mut self, request: ChannelCreate) -> Result<Value, ErrorCode> {
        self.workspace(self.bot(request.bot_id)?)?;
        if matches!(request.config, ChannelConfig::Builtin) {
            return Err(ErrorCode::BotConflict);
        }
        if let Some(channel) = self
            .catalog
            .channels
            .iter()
            .find(|channel| channel.id == request.channel_id)
        {
            if channel.bot_id == request.bot_id
                && channel.config == request.config
                && channel.enabled == request.enabled
                && !channel.deleted
            {
                return Ok(json!({"channel":channel,"created":false}));
            }
            return Err(ErrorCode::IdempotencyConflict);
        }
        if self
            .catalog
            .channels
            .iter()
            .filter(|channel| channel.bot_id == request.bot_id && !channel.deleted)
            .count()
            >= 16
        {
            return Err(ErrorCode::ResourceExhausted);
        }
        let channel = Channel {
            id: request.channel_id,
            bot_id: request.bot_id,
            config: request.config,
            enabled: request.enabled,
            cursor: None,
            deleted: false,
        };
        let mut catalog = self.catalog.clone();
        catalog.channels.push(channel.clone());
        self.save_catalog(catalog)?;
        Ok(json!({"channel":channel,"created":true}))
    }
    pub(super) fn update_channel(&mut self, request: ChannelUpdate) -> Result<Value, ErrorCode> {
        self.workspace(self.bot(request.bot_id)?)?;
        let mut channel = self.channel(request.bot_id, request.channel_id)?.clone();
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
        Ok(json!({"channel":channel}))
    }
    pub(super) fn delete_channel(&mut self, request: ChannelRequest) -> Result<Value, ErrorCode> {
        let channel = self.channel(request.bot_id, request.channel_id)?;
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
