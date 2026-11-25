use domain::ChannelId;
use domain::bot::{BotChat, ChannelConfig};
use model::ErrorCode;
use serde_json::{Value, json};

use super::State;
use crate::protocol::{ChatUpdate, WorkspaceRequest};

impl State {
    /// Register only chat identity before applying its reception policy; ignored bodies are
    /// never persisted and do not create a model conversation.
    pub(crate) fn admit_chat(
        &mut self,
        channel: ChannelId,
        peer: String,
        group: bool,
        mentioned: bool,
    ) -> Result<Value, ErrorCode> {
        let configured = self
            .catalog
            .channels
            .iter()
            .find(|c| c.id == channel && !c.deleted)
            .ok_or(ErrorCode::BotNotFound)?;
        let (ChannelConfig::Feishu { .. } | ChannelConfig::Lark { .. }) = &configured.config else {
            return Err(ErrorCode::InvalidMessage);
        };
        self.workspace(self.bot(configured.bot_id)?)?;
        if !configured.enabled || !model::valid_id(&peer) {
            return Err(ErrorCode::BotConflict);
        }
        let chat = if let Some(chat) = self
            .catalog
            .chats
            .iter()
            .find(|c| c.channel_id == channel && c.peer == peer)
        {
            if chat.group != group {
                return Err(ErrorCode::InvalidMessage);
            }
            chat.clone()
        } else {
            if self.catalog.chats.len() >= 4096 {
                return Err(ErrorCode::ResourceExhausted);
            }
            let chat = BotChat {
                channel_id: channel,
                peer,
                group,
                enabled: true,
                listen_all_messages: false,
            };
            let mut catalog = self.catalog.clone();
            catalog.chats.push(chat.clone());
            self.save_catalog(catalog)?;
            chat
        };
        Ok(json!({"accepted":chat.enabled && (!group || (mentioned || chat.listen_all_messages))}))
    }

    pub(super) fn list_chats(&self, request: &WorkspaceRequest) -> Result<Value, ErrorCode> {
        let bot = self.current(&request.workspace_id)?.id;
        let chats: Vec<_> = self
            .catalog
            .chats
            .iter()
            .filter(|chat| self.channel(bot, chat.channel_id).is_ok_and(|c| !c.deleted))
            .collect();
        Ok(json!({"chats":chats}))
    }

    pub(super) fn update_chat(&mut self, request: &ChatUpdate) -> Result<Value, ErrorCode> {
        self.channel(self.current(&request.workspace_id)?.id, request.channel_id)?;
        let mut catalog = self.catalog.clone();
        let chat = catalog
            .chats
            .iter_mut()
            .find(|c| c.channel_id == request.channel_id && c.peer == request.peer)
            .ok_or(ErrorCode::BotNotFound)?;
        chat.enabled = request.enabled;
        chat.listen_all_messages = chat.group && request.listen_all_messages;
        let chat = chat.clone();
        self.save_catalog(catalog)?;
        Ok(json!({"chat":chat}))
    }
}
