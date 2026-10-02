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
    core::{Author, Avatar, Message, Source},
    database::{Database, Link},
};

mod markdown;
mod media;
mod session;

use markdown::{Mention, Mentions};
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
    avatars: media::Avatars,
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
            avatars: media::Avatars::default(),
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
            let mentions = mentions(context, &message.content).await?;
            for id in media::send(&context.client, target, message, reply_to, &mentions).await? {
                context
                    .database
                    .add_link(message.id, &link(context, peer, id))
                    .await?;
            }
        }
        MessageEvent::Edit(message) | MessageEvent::Reactions(message) => {
            // the platform message there is the user's own, which the bot cannot edit
            if context
                .database
                .message_origin(message.id)
                .await?
                .as_deref()
                == Some(&context.name)
            {
                return Ok(());
            }
            // only the first message of a bridged message carries its text
            if let Some(&id) = platform_ids(context, message.id, peer).await?.first() {
                let mentions = mentions(context, &message.content).await?;
                context
                    .client
                    .edit_message(target, id, render(context, message, &mentions))
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

fn render(context: &Context, message: &Message, mentions: &Mentions) -> InputMessage {
    let (text, entities) = markdown::bridged(
        &message.author.full_name(Some(0)),
        &message.content,
        message.reaction_summary(&context.name).as_deref(),
        mentions,
    );
    InputMessage::new().text(text).fmt_entities(entities)
}

/// Looks up the users that `content` mentions like `@tg/name`, if this backend has seen them.
async fn mentions(context: &Context, content: &str) -> sqlx::Result<Mentions> {
    let mut mentions = Mentions::new();
    for (_, name) in Source::Telegram.mentions(content) {
        let Some((id, display_name)) = context.database.find_user(&context.name, name).await?
        else {
            continue;
        };
        let Some(peer) = id.parse().ok().and_then(PeerId::user) else {
            continue;
        };
        // a mention without the user's access hash makes Telegram reject the whole message
        let Some(user) = context.session.peer_ref(peer).await.ok().flatten() else {
            continue;
        };
        let label = if name.parse::<i64>().is_ok() {
            display_name.unwrap_or_else(|| name.to_owned())
        } else {
            format!("@{name}")
        };
        mentions.insert(
            name.to_lowercase(),
            Mention {
                label,
                user: user.into(),
            },
        );
    }
    Ok(mentions)
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
        Update::Raw(raw) => receive_reactions(context, chats, &raw.raw).await,
        _ => Ok(()),
    }
}

/// Telegram only sends reaction updates to bots that are admins in the chat.
async fn receive_reactions(
    context: &Context,
    chats: &[Chat],
    update: &tl::enums::Update,
) -> TaskResult {
    let (peer, message_id) = match update {
        tl::enums::Update::BotMessageReaction(update) => (&update.peer, update.msg_id),
        tl::enums::Update::BotMessageReactions(update) => (&update.peer, update.msg_id),
        _ => return Ok(()),
    };
    let peer = PeerId::from(peer.clone());
    let targets: Vec<_> = chats.iter().filter(|c| c.peer == peer).collect();
    if targets.is_empty() {
        return Ok(());
    }
    let database = &context.database;
    let Some(id) = database
        .find_message(&link(context, peer, message_id))
        .await?
    else {
        return Ok(());
    };

    match update {
        // one user changed their reactions
        tl::enums::Update::BotMessageReaction(update) => {
            for emoji in update.old_reactions.iter().filter_map(emoji_name) {
                database.add_reaction(id, &context.name, &emoji, -1).await?;
            }
            for emoji in update.new_reactions.iter().filter_map(emoji_name) {
                database.add_reaction(id, &context.name, &emoji, 1).await?;
            }
        }
        // anonymous totals, as in channels
        tl::enums::Update::BotMessageReactions(update) => {
            let counts: Vec<_> = update
                .reactions
                .iter()
                .filter_map(|tl::enums::ReactionCount::Count(count)| {
                    Some((emoji_name(&count.reaction)?, i64::from(count.count)))
                })
                .collect();
            database.set_reactions(id, &context.name, &counts).await?;
        }
        _ => {}
    }

    let Some(core) = database.message(id).await? else {
        return Ok(());
    };
    for chat in targets {
        chat.group.send(MessageEvent::Reactions(core.clone()));
    }
    Ok(())
}

fn emoji_name(reaction: &tl::enums::Reaction) -> Option<String> {
    match reaction {
        tl::enums::Reaction::Emoji(emoji) => Some(emoji.emoticon.clone()),
        tl::enums::Reaction::CustomEmoji(_) => Some(":emoji:".to_owned()),
        tl::enums::Reaction::Paid => Some("⭐".to_owned()),
        tl::enums::Reaction::Empty => None,
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
    if let Some(Peer::User(user)) = message.sender() {
        let id = user.id().bare_id_unchecked().to_string();
        context
            .database
            .remember_user(&context.name, &core.author, &id)
            .await?;
    }
    for chat in targets {
        let group = &chat.group;
        let id = context
            .database
            .create_message(&group.name, &group.backend_name, &core)
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
    context.database.set_content(id, &core.content).await?;
    let stored = context
        .database
        .message(id)
        .await?
        .unwrap_or(Message { id, ..core });
    for chat in chats.iter().filter(|c| c.peer == message.peer_id()) {
        chat.group.send(MessageEvent::Edit(stored.clone()));
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
    let mut author = author_of(message);
    if download && let Some(peer) = message.sender().or_else(|| message.peer()) {
        author.avatar = match context.avatars.get(&context.client, peer).await {
            Ok(file) => file.map(Avatar::File),
            Err(e) => {
                warn!(
                    "TelegramBackend '{}' failed to download an avatar: {e}",
                    context.name
                );
                None
            }
        };
    }
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
        reactions: vec![],
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
