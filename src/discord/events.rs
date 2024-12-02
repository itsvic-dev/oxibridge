use serenity::{
    all::{ChannelId, Context, EventHandler, GuildId, Message, MessageId, MessageUpdateEvent},
    async_trait,
};
use tracing::*;

use crate::{
    broadcast::{MessageEvent, Source},
    config::GroupConfig,
};

use super::{parsers::to_core_message, BotEventHandler};

#[async_trait]
impl EventHandler for BotEventHandler {
    #[instrument(skip_all)]
    async fn message(&self, _ctx: Context, msg: Message) {
        if msg.author.bot || msg.author.system {
            return;
        }

        // find the respective group
        let group: Vec<GroupConfig> = self
            .config
            .groups
            .clone()
            .into_iter()
            .filter(|g| g.discord_channel == msg.channel_id.get())
            .collect();

        let group = match group.first() {
            Some(group) => group,
            None => return,
        };

        let core_msg = match to_core_message(&msg).await {
            Ok(core_msg) => core_msg,
            Err(why) => {
                error!(?why, "Failed to parse into core message");
                return;
            }
        };

        // only cache dsc->core
        // for discord messages, we obviously won't be handling external events lol
        let mut cache = self.cache.lock().await;
        cache.dsc_core_cache.insert(msg.id, core_msg.id);

        if let Err(why) = self
            .broadcaster
            .lock()
            .await
            .broadcast(group, &MessageEvent::Create(core_msg), Source::Discord)
            .await
        {
            error!(?why, "Failed to broadcast message");
        }
    }

    #[instrument(skip_all)]
    async fn message_update(
        &self,
        _ctx: Context,
        _old: Option<Message>,
        _new: Option<Message>,
        event: MessageUpdateEvent,
    ) {
        if event
            .author
            .map_or(false, |author| author.bot || author.system)
        {
            return;
        }

        // find the respective group
        let group: Vec<GroupConfig> = self
            .config
            .groups
            .clone()
            .into_iter()
            .filter(|g| g.discord_channel == event.channel_id.get())
            .collect();

        let group = match group.first() {
            Some(group) => group,
            None => return,
        };

        let cache = self.cache.lock().await;
        let core_id = match cache.dsc_core_cache.get(&event.id) {
            Some(id) => *id,
            None => {
                error!("Could not find edited message in local cache");
                return;
            }
        };

        if let Err(why) = self
            .broadcaster
            .lock()
            .await
            .broadcast(
                group,
                &MessageEvent::Update(core_id, event.content.unwrap_or_default()),
                Source::Discord,
            )
            .await
        {
            error!(?why, "Failed to broadcast message");
        }
    }

    async fn message_delete(
        &self,
        _ctx: Context,
        channel_id: ChannelId,
        deleted_id: MessageId,
        _guild_id: Option<GuildId>,
    ) {
        // find the respective group
        let group: Vec<GroupConfig> = self
            .config
            .groups
            .clone()
            .into_iter()
            .filter(|g| g.discord_channel == channel_id.get())
            .collect();

        let group = match group.first() {
            Some(group) => group,
            None => return,
        };

        let core_id = match self.cache.lock().await.dsc_core_cache.get(&deleted_id) {
            Some(id) => *id,
            None => return,
        };

        if let Err(why) = self
            .broadcaster
            .lock()
            .await
            .broadcast(group, &MessageEvent::Delete(core_id), Source::Discord)
            .await
        {
            error!(?why, "Failed to broadcast message");
        }
    }
}
