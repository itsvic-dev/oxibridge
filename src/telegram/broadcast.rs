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

use super::TelegramBridge;

#[async_trait]
impl BroadcastReceiver for TelegramBridge {
    async fn receive(&self, message: &core::Message) -> Result<()> {
        let text = format!("*{}*\n{}", &message.author.full_name(), &message.content)
            .replace("(", "\\(")
            .replace(")", "\\)")
            .replace("[", "\\[")
            .replace("]", "\\]")
            .replace(".", "\\.");

        if message.attachments.len() > 0 {
            todo!();
        } else {
            self.bot
                .send_message(Recipient::Id(ChatId(self.chat_id)), &text)
                .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                .await?;
        }

        Ok(())
    }

    fn get_receiver_source(&self) -> Source {
        Source::Telegram
    }
}
