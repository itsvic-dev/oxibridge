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

mod markdown;
mod media;
mod session;

use session::StoredSession;

type TaskResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

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
            for id in media::send(&context.client, target, message, reply_to).await? {
                context
                    .database
                    .add_link(message.id, &link(context, peer, id))
                    .await?;
            }
        }
        MessageEvent::Edit(message) => {
            // only the first message of a bridged message carries its text
            if let Some(&id) = platform_ids(context, message.id, peer).await?.first() {
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
    let (text, entities) = markdown::bridged(&message.author.full_name(Some(0)), &message.content);
    InputMessage::new().text(text).fmt_entities(entities)
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
    let targets: Vec<_> = chats
        .iter()
        .filter(|c| c.peer == message.peer_id())
        .collect();
    if targets.is_empty() {
        return Ok(());
    }
    let link = link(context, message.peer_id(), message.id());
    // catching up after a restart can replay messages that were already bridged
    if context.database.find_message(&link).await?.is_some() {
        return Ok(());
    }

    let core = to_core(context, message, true).await?;
    if core.content.is_empty() && core.attachments.is_empty() {
        return Ok(());
    }
    for chat in targets {
        let id = context
            .database
            .create_message(&chat.group.name, &(&core.author).into())
            .await?;
        context.database.add_link(id, &link).await?;
        chat.group
            .send(MessageEvent::Create(Message { id, ..core.clone() }));
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

    // edits on the other side only change the text, so the media is not downloaded again
    let core = to_core(context, message, false).await?;
    for chat in chats.iter().filter(|c| c.peer == message.peer_id()) {
        chat.group
            .send(MessageEvent::Edit(Message { id, ..core.clone() }));
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

/// Converts `message` to a core message, with ID 0 for the caller to replace.
async fn to_core(context: &Context, message: &TgMessage, download: bool) -> TaskResult<Message> {
    let author = author_of(message);
    let media = media::incoming(
        &context.client,
        message,
        &author.full_name(Some(0)),
        download,
    )
    .await?;
    let text = markdown::to_markdown(
        message.text(),
        message.fmt_entities().map_or(&[], Vec::as_slice),
    );
    let content = [
        forward_header(context, message).await,
        media.label,
        Some(text),
    ]
    .into_iter()
    .flatten()
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join("\n");

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
        id: 0,
        author,
        content,
        attachments: media.attachments,
        in_reply_to,
        reply_author,
    })
}

async fn forward_header(context: &Context, message: &TgMessage) -> Option<String> {
    let tl::enums::MessageFwdHeader::Header(header) = message.forward_header()?;
    let name = match (header.from_name, header.from_id) {
        (Some(name), _) => Some(name),
        (None, Some(peer)) => {
            let peer = peer_ref(context, PeerId::from(peer)).await;
            context
                .client
                .resolve_peer(peer)
                .await
                .ok()
                .and_then(|peer| peer_name(&peer))
        }
        (None, None) => None,
    };
    let name = name.unwrap_or_else(|| "someone".to_owned());
    Some(match header.post_author {
        Some(signature) => format!("*Forwarded from {name} ({signature})*"),
        None => format!("*Forwarded from {name}*"),
    })
}

fn peer_name(peer: &Peer) -> Option<String> {
    match peer {
        Peer::User(user) => Some(user.full_name()),
        peer => peer.name().map(str::to_owned),
    }
}

fn author_of(message: &TgMessage) -> Author {
    let peer = message.sender().or_else(|| message.peer());
    let display_name = peer.and_then(peer_name);
    let username = peer.and_then(Peer::username).map_or_else(
        || message.sender_id().unwrap_or(message.peer_id()).to_string(),
        str::to_owned,
    );

    Author {
        display_name,
        username,
        avatar: None,
        source: Source::Telegram,
    }
}
