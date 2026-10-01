use std::{error::Error, sync::Arc};

use grammers_client::{
    Client, SenderPool,
    client::UpdatesConfiguration,
    message::{InputMessage, Message as TgMessage},
    peer::Peer,
    session::{
        Session,
        types::{PeerId, PeerKind, PeerRef},
    },
    tl,
    update::{MessageDeletion, Update},
};
use log::{debug, warn};
use serde::{Deserialize, Serialize};
use tokio::task::JoinSet;

use crate::{
    backends::{BackendGroup, MessageEvent},
    core::{Author, Message, Source},
    database::{Database, Link},
};

mod session;

use session::StoredSession;

type TaskResult = Result<(), Box<dyn Error + Send + Sync>>;

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    /// Bot token from @BotFather.
    pub token: String,
    /// API ID from <https://my.telegram.org>.
    pub api_id: i32,
    /// API hash from <https://my.telegram.org>.
    pub api_hash: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GroupConfig {
    // Bot API chat ID, for example -1001234567890
    chat: i64,
}

#[derive(Clone)]
struct Chat {
    group: BackendGroup,
    peer: PeerId,
}

#[derive(Clone)]
struct Context {
    name: String,
    client: Client,
    session: Arc<StoredSession>,
    database: Database,
}

pub struct TelegramBackend {
    name: String,
    token: String,
    api_id: i32,
    api_hash: String,
    chats: Vec<Chat>,
    database: Database,
}

impl TelegramBackend {
    pub fn new(
        name: &str,
        config: &Config,
        group_configs: &[BackendGroup],
        database: Database,
    ) -> Result<Self, Box<dyn Error>> {
        let chats = group_configs
            .iter()
            .map(|group| {
                let context = format!("backend '{name}' in group '{}'", group.name);
                let options: GroupConfig = group
                    .config
                    .options()
                    .map_err(|e| format!("{context}: {e}"))?;
                let peer = PeerId::from_bot_api_dialog_id(options.chat)
                    .ok_or_else(|| format!("{context}: invalid chat ID {}", options.chat))?;
                Ok(Chat {
                    group: group.clone(),
                    peer,
                })
            })
            .collect::<Result<Vec<_>, Box<dyn Error>>>()?;

        Ok(Self {
            name: name.to_owned(),
            token: config.token.clone(),
            api_id: config.api_id,
            api_hash: config.api_hash.clone(),
            chats,
            database,
        })
    }
}

#[async_trait::async_trait]
impl super::Backend for TelegramBackend {
    async fn start(&self, tasks: &mut JoinSet<()>) -> Result<(), Box<dyn Error>> {
        let session = Arc::new(StoredSession::load(self.database.clone(), &self.name).await?);
        let SenderPool {
            runner,
            updates,
            handle,
        } = SenderPool::new(Arc::clone(&session), self.api_id);
        tasks.spawn(runner.run());

        let client = Client::new(handle);
        if !client.is_authorized().await? {
            client.bot_sign_in(&self.token, &self.api_hash).await?;
        }
        let me = client.get_me().await?;
        debug!(
            "TelegramBackend '{}' signed in as @{}",
            self.name,
            me.username().unwrap_or_default()
        );

        let context = Context {
            name: self.name.clone(),
            client: client.clone(),
            session,
            database: self.database.clone(),
        };

        for chat in self.chats.iter().filter(|c| !c.group.config.readonly) {
            let mut rx = chat.group.subscribe();
            let context = context.clone();
            let peer = chat.peer;
            tasks.spawn(async move {
                while let Some(message) = rx.recv().await {
                    if let Err(e) = deliver(&context, peer, &message.event).await {
                        warn!(
                            "TelegramBackend '{}' failed to deliver to {}: {e}",
                            context.name,
                            chat_key(peer)
                        );
                    }
                }
            });
        }

        let chats: Vec<_> = self
            .chats
            .iter()
            .filter(|c| !c.group.config.writeonly)
            .cloned()
            .collect();
        let mut stream = client
            .stream_updates(
                updates,
                UpdatesConfiguration {
                    catch_up: true,
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| -> Box<dyn Error> { e })?;
        tasks.spawn(async move {
            loop {
                let update = match stream.next().await {
                    Ok(update) => update,
                    Err(e) => {
                        warn!("TelegramBackend '{}' stopped receiving: {e}", context.name);
                        break;
                    }
                };
                if let Err(e) = receive(&context, &chats, update).await {
                    warn!(
                        "TelegramBackend '{}' failed to handle an update: {e}",
                        context.name
                    );
                }
                if let Err(e) = stream.sync_update_state().await {
                    warn!(
                        "TelegramBackend '{}' failed to save its update state: {e}",
                        context.name
                    );
                }
            }
        });

        Ok(())
    }
}

/// Bot API form of a chat ID. Used as the chat of links and in the config.
fn chat_key(peer: PeerId) -> String {
    peer.bot_api_dialog_id_unchecked().to_string()
}

fn link(context: &Context, peer: PeerId, message_id: i32) -> Link {
    Link {
        backend: context.name.clone(),
        chat: chat_key(peer),
        platform_id: message_id.to_string(),
    }
}

async fn peer_ref(context: &Context, peer: PeerId) -> PeerRef {
    context
        .session
        .peer_ref(peer)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| peer.to_ambient_ref())
}

async fn platform_ids(context: &Context, message_id: i64, peer: PeerId) -> sqlx::Result<Vec<i32>> {
    let chat = chat_key(peer);
    Ok(context
        .database
        .links(message_id, &context.name)
        .await?
        .into_iter()
        .filter(|link| link.chat == chat)
        .filter_map(|link| link.platform_id.parse().ok())
        .collect())
}

async fn deliver(context: &Context, peer: PeerId, event: &MessageEvent) -> TaskResult {
    let target = peer_ref(context, peer).await;
    match event {
        MessageEvent::Create(message) => {
            let reply_to = match message.in_reply_to {
                Some(reply) => platform_ids(context, reply, peer).await?.first().copied(),
                None => None,
            };
            let sent = context
                .client
                .send_message(target, render(message).reply_to(reply_to))
                .await?;
            context
                .database
                .add_link(message.id, &link(context, peer, sent.id()))
                .await?;
        }
        MessageEvent::Edit(message) => {
            for id in platform_ids(context, message.id, peer).await? {
                context
                    .client
                    .edit_message(target, id, render(message))
                    .await?;
            }
        }
        MessageEvent::Delete(message_id) => {
            let ids = platform_ids(context, *message_id, peer).await?;
            // forget the links first, so the delete update Telegram sends back finds nothing to bridge
            context
                .database
                .remove_links(*message_id, &context.name)
                .await?;
            if !ids.is_empty() {
                context.client.delete_messages(target, &ids).await?;
            }
        }
    }
    Ok(())
}

fn render(message: &Message) -> InputMessage {
    let (text, entities) = format_text(message);
    InputMessage::new().text(text).fmt_entities(entities)
}

/// Formats a bridged message as the author's name in bold, then the content on the next line.
fn format_text(message: &Message) -> (String, Vec<tl::enums::MessageEntity>) {
    let name = message.author.full_name(Some(0));
    let length = i32::try_from(name.encode_utf16().count()).unwrap_or(i32::MAX);
    let bold = tl::types::MessageEntityBold { offset: 0, length }.into();
    (format!("{name}\n{}", message.content), vec![bold])
}

async fn receive(context: &Context, chats: &[Chat], update: Update) -> TaskResult {
    match update {
        Update::NewMessage(message) if !message.outgoing() => {
            receive_new(context, chats, &message).await
        }
        Update::MessageEdited(message) if !message.outgoing() => {
            receive_edit(context, chats, &message).await
        }
        Update::MessageDeleted(deletion) => receive_delete(context, chats, &deletion).await,
        _ => Ok(()),
    }
}

async fn receive_new(context: &Context, chats: &[Chat], message: &TgMessage) -> TaskResult {
    if message.text().is_empty() {
        return Ok(());
    }
    let link = link(context, message.peer_id(), message.id());
    // catching up after a restart can replay messages that were already bridged
    if context.database.find_message(&link).await?.is_some() {
        return Ok(());
    }

    let author = author_of(message);
    for chat in chats.iter().filter(|c| c.peer == message.peer_id()) {
        let id = context
            .database
            .create_message(&chat.group.name, &(&author).into())
            .await?;
        context.database.add_link(id, &link).await?;
        let core = to_core(context, message, id, author.clone()).await?;
        chat.group.send(MessageEvent::Create(core));
    }
    Ok(())
}

async fn receive_edit(context: &Context, chats: &[Chat], message: &TgMessage) -> TaskResult {
    let Some(id) = context
        .database
        .find_message(&link(context, message.peer_id(), message.id()))
        .await?
    else {
        return Ok(());
    };

    for chat in chats.iter().filter(|c| c.peer == message.peer_id()) {
        let core = to_core(context, message, id, author_of(message)).await?;
        chat.group.send(MessageEvent::Edit(core));
    }
    Ok(())
}

async fn receive_delete(
    context: &Context,
    chats: &[Chat],
    deletion: &MessageDeletion,
) -> TaskResult {
    for &message_id in deletion.messages() {
        let found = match deletion.channel_id().and_then(PeerId::channel) {
            Some(peer) => context
                .database
                .find_message(&link(context, peer, message_id))
                .await?
                .map(|id| (id, chat_key(peer)))
                .into_iter()
                .collect(),
            // message IDs outside channels are unique per account, but channels reuse them
            None => context
                .database
                .find_messages_in_any_chat(&context.name, &message_id.to_string())
                .await?
                .into_iter()
                .filter(|(_, chat)| {
                    chats
                        .iter()
                        .any(|c| chat_key(c.peer) == *chat && c.peer.kind() != PeerKind::Channel)
                })
                .collect::<Vec<_>>(),
        };

        for (id, chat) in found {
            context.database.remove_links(id, &context.name).await?;
            for target in chats.iter().filter(|c| chat_key(c.peer) == chat) {
                target.group.send(MessageEvent::Delete(id));
            }
        }
    }
    Ok(())
}

async fn to_core(
    context: &Context,
    message: &TgMessage,
    id: i64,
    author: Author,
) -> sqlx::Result<Message> {
    let in_reply_to = match message.reply_to_message_id() {
        Some(reply) => {
            context
                .database
                .find_message(&link(context, message.peer_id(), reply))
                .await?
        }
        None => None,
    };
    let reply_author = match in_reply_to {
        Some(reply) => context.database.message_author(reply).await?,
        None => None,
    };

    Ok(Message {
        id,
        author,
        content: message.text().to_owned(),
        attachments: vec![],
        in_reply_to,
        reply_author,
    })
}

fn author_of(message: &TgMessage) -> Author {
    let peer = message.sender().or_else(|| message.peer());
    let display_name = peer.and_then(|peer| match peer {
        Peer::User(user) => Some(user.full_name()),
        peer => peer.name().map(str::to_owned),
    });
    let username = peer.and_then(Peer::username).map_or_else(
        || message.sender_id().unwrap_or(message.peer_id()).to_string(),
        str::to_owned,
    );

    Author {
        display_name,
        username,
        source: Source::Telegram,
    }
}

#[cfg(test)]
mod tests {
    use grammers_client::tl;

    use super::format_text;
    use crate::core::{Author, Message, Source};

    fn message(display_name: &str, content: &str) -> Message {
        Message {
            id: 1,
            author: Author {
                display_name: Some(display_name.to_owned()),
                username: "vic".to_owned(),
                source: Source::Irc,
            },
            content: content.to_owned(),
            attachments: vec![],
            in_reply_to: None,
            reply_author: None,
        }
    }

    #[test]
    fn puts_the_author_in_bold_above_the_content() {
        let (text, entities) = format_text(&message("Vic", "hello"));
        assert_eq!(text, "Vic (@irc/vic)\nhello");
        assert_eq!(
            entities,
            vec![
                tl::types::MessageEntityBold {
                    offset: 0,
                    length: 14
                }
                .into()
            ]
        );
    }

    #[test]
    fn measures_the_author_in_utf16_units() {
        let (_, entities) = format_text(&message("🦀", "hi"));
        assert_eq!(
            entities,
            vec![
                tl::types::MessageEntityBold {
                    offset: 0,
                    length: 13
                }
                .into()
            ]
        );
    }
}
