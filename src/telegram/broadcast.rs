use crate::{
    broadcast::{BroadcastReceiver, Source},
    core,
};
use color_eyre::eyre::Result;
use serenity::async_trait;
use teloxide::{
    payloads::SendMessageSetters,
    prelude::Requester,
    types::{ChatId, Recipient},
};

use super::{entities::to_string_with_entities, TelegramBridge};

#[async_trait]
impl BroadcastReceiver for TelegramBridge {
    async fn receive(&self, message: &core::Message) -> Result<()> {
        let text = format!("**{}**\n{}", &message.author.full_name(), &message.content);

        let parsed = to_string_with_entities(&text);

        if !message.attachments.is_empty() {
            todo!();
        } else {
            self.bot
                .send_message(Recipient::Id(ChatId(self.chat_id)), parsed.0)
                .entities(parsed.1)
                .await?;
        }

        Ok(())
    }

    fn get_receiver_source(&self) -> Source {
        Source::Telegram
    }
}
