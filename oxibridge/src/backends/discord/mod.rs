use std::{error::Error, sync::Arc};

use async_tempfile::TempFile;
use log::{debug, warn};
use serde::{Deserialize, Serialize};
use serenity::{
    all::{
        ChannelId, Context as SerenityContext, CreateAllowedMentions, CreateAttachment,
        CreateWebhook, EditWebhookMessage, EventHandler, ExecuteWebhook, GatewayIntents, GuildId,
        Http, Message as DiscordMessage, MessageId, MessageReferenceKind, MessageType,
        MessageUpdateEvent, Reaction as DiscordReaction, ReactionType, User, UserId, Webhook,
    },
    async_trait,
};
use tokio::{io::AsyncWriteExt, task::JoinSet};

use crate::{
    backends::{BackendGroup, MessageEvent},
    core::{Attachment, Author, Avatar, Message, Source},
    database::{Database, Link},
    storage::R2Storage,
};

mod content;

type TaskResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

// Discord's upload limit for bots in servers without boosts
const MAX_UPLOAD_BYTES: u64 = 10 * 1024 * 1024;
const MAX_DOWNLOAD_BYTES: u32 = 50 * 1024 * 1024;
const WEBHOOK_NAME: &str = "Oxibridge";

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    /// Bot token from <https://discord.com/developers/applications>.
    pub token: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GroupConfig {
    channel: u64,
}

#[derive(Clone)]
struct Chat {
    group: BackendGroup,
    channel: ChannelId,
}

#[derive(Clone)]
struct Context {
    name: String,
    database: Database,
    storage: Option<Arc<R2Storage>>,
}

pub struct DiscordBackend {
    name: String,
    token: String,
    chats: Vec<Chat>,
    database: Database,
    storage: Option<Arc<R2Storage>>,
}

impl DiscordBackend {
    pub fn new(
        name: &str,
        config: &Config,
        group_configs: &[BackendGroup],
        database: Database,
        storage: Option<Arc<R2Storage>>,
    ) -> Result<Self, Box<dyn Error>> {
        let chats = group_configs
            .iter()
            .map(|group| {
                let options: GroupConfig = group
                    .config
                    .options()
                    .map_err(|e| format!("backend '{name}' in group '{}': {e}", group.name))?;
                Ok(Chat {
                    group: group.clone(),
                    channel: ChannelId::new(options.channel),
                })
            })
            .collect::<Result<Vec<_>, Box<dyn Error>>>()?;

        Ok(Self {
            name: name.to_owned(),
            token: config.token.clone(),
            chats,
            database,
            storage,
        })
    }
}

#[async_trait]
impl super::Backend for DiscordBackend {
    async fn start(&self, tasks: &mut JoinSet<()>) -> Result<(), Box<dyn Error>> {
        let context = Context {
            name: self.name.clone(),
            database: self.database.clone(),
            storage: self.storage.clone(),
        };
        let handler = Handler {
            context: context.clone(),
            chats: self
                .chats
                .iter()
                .filter(|c| !c.group.config.writeonly)
                .cloned()
                .collect(),
        };
        let intents = GatewayIntents::GUILD_MESSAGES
            | GatewayIntents::GUILD_MESSAGE_REACTIONS
            | GatewayIntents::MESSAGE_CONTENT;
        let mut client = serenity::Client::builder(&self.token, intents)
            .event_handler(handler)
            .await?;
        let http = Arc::clone(&client.http);
        let me = http.get_current_user().await?;
        debug!("DiscordBackend '{}' signed in as {}", self.name, me.name);

        for chat in self.chats.iter().filter(|c| !c.group.config.readonly) {
            let webhook = find_webhook(&http, chat.channel, &me).await?;
            let mut rx = chat.group.subscribe();
            let context = context.clone();
            let http = Arc::clone(&http);
            let channel = chat.channel;
            tasks.spawn(async move {
                while let Some(message) = rx.recv().await {
                    if let Err(e) =
                        deliver(&context, &http, &webhook, channel, &message.event).await
                    {
                        warn!(
                            "DiscordBackend '{}' failed to deliver to {channel}: {e}",
                            context.name
                        );
                    }
                }
            });
        }

        let name = self.name.clone();
        tasks.spawn(async move {
            if let Err(e) = client.start().await {
                warn!("DiscordBackend '{name}' stopped: {e}");
            }
        });
        Ok(())
    }
}

/// Finds the webhook this bot made in `channel` before, or makes one.
async fn find_webhook(
    http: &Http,
    channel: ChannelId,
    me: &User,
) -> Result<Webhook, Box<dyn Error>> {
    let existing = channel.webhooks(http).await?.into_iter().find(|webhook| {
        webhook.token.is_some() && webhook.user.as_ref().is_some_and(|user| user.id == me.id)
    });
    match existing {
        Some(webhook) => Ok(webhook),
        None => Ok(channel
            .create_webhook(http, CreateWebhook::new(WEBHOOK_NAME))
            .await?),
    }
}

fn link(context: &Context, channel: ChannelId, message: MessageId) -> Link {
    Link {
        backend: context.name.clone(),
        chat: channel.to_string(),
        platform_id: message.to_string(),
    }
}

async fn message_ids(
    context: &Context,
    message_id: i64,
    channel: ChannelId,
) -> sqlx::Result<Vec<MessageId>> {
    let chat = channel.to_string();
    Ok(context
        .database
        .links(message_id, &context.name)
        .await?
        .into_iter()
        .filter(|link| link.chat == chat)
        .filter_map(|link| link.platform_id.parse().ok().map(MessageId::new))
        .collect())
}

async fn deliver(
    context: &Context,
    http: &Http,
    webhook: &Webhook,
    channel: ChannelId,
    event: &MessageEvent,
) -> TaskResult {
    match event {
        MessageEvent::Create(message) => {
            let (text, mentioned) = text_for(context, http, channel, message).await?;
            let (files, too_large) = uploads(message).await?;
            let text = text + &too_large;
            let mut pieces = content::split(&text).into_iter();
            let username = content::webhook_username(&message.author.full_name(None));
            let allowed_mentions = CreateAllowedMentions::new().users(mentioned);
            let avatar = avatar_url(context, &message.author).await;
            let base = || {
                let builder = ExecuteWebhook::new()
                    .username(&username)
                    .allowed_mentions(allowed_mentions.clone());
                match &avatar {
                    Some(url) => builder.avatar_url(url),
                    None => builder,
                }
            };

            let mut builder = base().add_files(files);
            if let Some(first) = pieces.next() {
                builder = builder.content(first);
            }
            let mut sent = vec![webhook.execute(http, true, builder).await?];
            for piece in pieces {
                let builder = base().content(piece);
                sent.push(webhook.execute(http, true, builder).await?);
            }
            for sent in sent.into_iter().flatten() {
                context
                    .database
                    .add_link(message.id, &link(context, channel, sent.id))
                    .await?;
            }
        }
        MessageEvent::Edit(message) | MessageEvent::Reactions(message) => {
            // the platform message there is the user's own, which the webhook cannot edit
            if context
                .database
                .message_origin(message.id)
                .await?
                .as_deref()
                == Some(&context.name)
            {
                return Ok(());
            }
            let ids = message_ids(context, message.id, channel).await?;
            if ids.is_empty() {
                return Ok(());
            }
            let (text, mentioned) = text_for(context, http, channel, message).await?;
            let text = match message.reaction_summary(&context.name) {
                Some(summary) if text.is_empty() => format!("-# {summary}"),
                Some(summary) => format!("{text}\n-# {summary}"),
                None => text,
            };
            for (id, piece) in ids.into_iter().zip(content::split(&text)) {
                let builder = EditWebhookMessage::new()
                    .content(piece)
                    .allowed_mentions(CreateAllowedMentions::new().users(mentioned.clone()));
                webhook.edit_message(http, id, builder).await?;
            }
        }
        MessageEvent::Delete(message_id) => {
            let ids = message_ids(context, *message_id, channel).await?;
            // forget the links first, so the delete events Discord sends back find nothing to bridge
            context
                .database
                .remove_links(*message_id, &context.name)
                .await?;
            for id in ids {
                webhook.delete_message(http, None, id).await?;
            }
        }
    }
    Ok(())
}

/// Returns a URL to the avatar of `author`. Avatar files need R2 storage to get one.
async fn avatar_url(context: &Context, author: &Author) -> Option<String> {
    match (&author.avatar, &context.storage) {
        (Some(Avatar::Url(url)), _) => Some(url.clone()),
        (Some(Avatar::File(file)), Some(storage)) => match storage.url(file, "image/jpeg").await {
            Ok(url) => Some(url),
            Err(e) => {
                warn!(
                    "DiscordBackend '{}' failed to upload an avatar: {e}",
                    context.name
                );
                None
            }
        },
        _ => None,
    }
}

/// Returns the bridged text with its reply header, and the users it may mention.
async fn text_for(
    context: &Context,
    http: &Http,
    channel: ChannelId,
    message: &Message,
) -> TaskResult<(String, Vec<UserId>)> {
    let (content, mut mentioned) = resolve_mentions(context, &message.content).await?;
    let replied = match message.in_reply_to {
        Some(reply) => message_ids(context, reply, channel).await?.first().copied(),
        None => None,
    };
    let Some(replied) = replied else {
        return Ok((content, mentioned));
    };

    let original = channel.message(http, replied).await?;
    let who = match &message.reply_author {
        Some(author) if author.source == Source::Discord => {
            mentioned.push(original.author.id);
            format!("<@{}>", original.author.id)
        }
        Some(author) => format!("**{}**", author.full_name(Some(0))),
        None => "a message".to_owned(),
    };
    let guild = original
        .guild_id
        .map_or_else(|| "@me".to_owned(), |guild| guild.to_string());
    let header =
        format!("-# Replying to {who}: https://discord.com/channels/{guild}/{channel}/{replied}\n");
    Ok((header + &content, mentioned))
}

/// Replaces `@dc/name` with a mention of that user, if this backend has seen them.
async fn resolve_mentions(context: &Context, content: &str) -> sqlx::Result<(String, Vec<UserId>)> {
    let mut text = String::new();
    let mut mentioned = vec![];
    let mut copied = 0;
    for (range, name) in Source::Discord.mentions(content) {
        let user = context.database.find_user(&context.name, name).await?;
        let Some(id) = user
            .and_then(|(id, _)| id.parse::<u64>().ok())
            .filter(|&id| id != 0)
        else {
            continue;
        };
        text.push_str(content.get(copied..range.start).unwrap_or_default());
        text.push_str(&format!("<@{id}>"));
        mentioned.push(UserId::new(id));
        copied = range.end;
    }
    text.push_str(content.get(copied..).unwrap_or_default());
    Ok((text, mentioned))
}

/// Reads the attachments to upload, and notes the ones over Discord's size limit.
async fn uploads(message: &Message) -> TaskResult<(Vec<CreateAttachment>, String)> {
    let mut files = vec![];
    let mut note = String::new();
    for attachment in &message.attachments {
        let path = attachment.file.file_path();
        if tokio::fs::metadata(path).await?.len() > MAX_UPLOAD_BYTES {
            note.push_str("\n-# ");
            note.push_str(&attachment.filename);
            note.push_str(" is too large for Discord");
            continue;
        }
        let name = if attachment.spoilered {
            format!("SPOILER_{}", attachment.filename)
        } else {
            attachment.filename.clone()
        };
        files.push(CreateAttachment::bytes(tokio::fs::read(path).await?, name));
    }
    Ok((files, note))
}

struct Handler {
    context: Context,
    chats: Vec<Chat>,
}

impl Handler {
    fn chats_in(&self, channel: ChannelId) -> impl Iterator<Item = &Chat> {
        self.chats
            .iter()
            .filter(move |chat| chat.channel == channel)
    }

    async fn receive_new(&self, http: &Http, message: &DiscordMessage) -> TaskResult {
        let bridged = message.author.bot || message.webhook_id.is_some();
        let regular = matches!(
            message.kind,
            MessageType::Regular | MessageType::InlineReply
        );
        if bridged || !regular || self.chats_in(message.channel_id).next().is_none() {
            return Ok(());
        }

        let core = to_core(&self.context, http, message).await?;
        if core.content.is_empty() && core.attachments.is_empty() {
            return Ok(());
        }
        let link = link(&self.context, message.channel_id, message.id);
        self.context
            .database
            .remember_user(
                &self.context.name,
                &core.author,
                &message.author.id.to_string(),
            )
            .await?;
        for chat in self.chats_in(message.channel_id) {
            let group = &chat.group;
            let id = self
                .context
                .database
                .create_message(&group.name, &group.backend_name, &core)
                .await?;
            self.context.database.add_link(id, &link).await?;
            chat.group
                .send(MessageEvent::Create(Message { id, ..core.clone() }))
                .await;
        }
        Ok(())
    }

    async fn receive_edit(&self, event: &MessageUpdateEvent) -> TaskResult {
        let (Some(author), Some(text)) = (&event.author, &event.content) else {
            return Ok(());
        };
        if author.bot {
            return Ok(());
        }
        let link = link(&self.context, event.channel_id, event.id);
        let Some(id) = self.context.database.find_message(&link).await? else {
            return Ok(());
        };

        let mentions = event.mentions.as_deref().unwrap_or_default();
        let content = content::to_core(text, &mention_names(mentions));
        self.context.database.set_content(id, &content).await?;
        let core = self.context.database.message(id).await?.unwrap_or(Message {
            id,
            author: author_of(author, None),
            content,
            attachments: vec![],
            in_reply_to: None,
            reply_author: None,
            reactions: vec![],
        });
        for chat in self.chats_in(event.channel_id) {
            chat.group.send(MessageEvent::Edit(core.clone())).await;
        }
        Ok(())
    }

    async fn receive_delete(&self, channel: ChannelId, message: MessageId) -> TaskResult {
        let link = link(&self.context, channel, message);
        let Some(id) = self.context.database.find_message(&link).await? else {
            return Ok(());
        };
        self.context
            .database
            .remove_links(id, &self.context.name)
            .await?;
        for chat in self.chats_in(channel) {
            chat.group.send(MessageEvent::Delete(id)).await;
        }
        Ok(())
    }

    async fn receive_reactions(
        &self,
        http: &Http,
        channel: ChannelId,
        message: MessageId,
    ) -> TaskResult {
        if self.chats_in(channel).next().is_none() {
            return Ok(());
        }
        let link = link(&self.context, channel, message);
        let Some(id) = self.context.database.find_message(&link).await? else {
            return Ok(());
        };
        let reactions: Vec<_> = channel
            .message(http, message)
            .await?
            .reactions
            .iter()
            .map(|reaction| {
                let count = i64::try_from(reaction.count).unwrap_or(i64::MAX);
                (emoji_name(&reaction.reaction_type), count)
            })
            .collect();
        let database = &self.context.database;
        database
            .set_reactions(id, &self.context.name, &reactions)
            .await?;
        let Some(core) = database.message(id).await? else {
            return Ok(());
        };
        for chat in self.chats_in(channel) {
            chat.group.send(MessageEvent::Reactions(core.clone())).await;
        }
        Ok(())
    }

    fn report(&self, result: TaskResult) {
        if let Err(e) = result {
            warn!(
                "DiscordBackend '{}' failed to handle an event: {e}",
                self.context.name
            );
        }
    }
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, ctx: SerenityContext, message: DiscordMessage) {
        self.report(self.receive_new(&ctx.http, &message).await);
    }

    async fn message_update(
        &self,
        _ctx: SerenityContext,
        _old: Option<DiscordMessage>,
        _new: Option<DiscordMessage>,
        event: MessageUpdateEvent,
    ) {
        self.report(self.receive_edit(&event).await);
    }

    async fn message_delete(
        &self,
        _ctx: SerenityContext,
        channel: ChannelId,
        message: MessageId,
        _guild: Option<GuildId>,
    ) {
        self.report(self.receive_delete(channel, message).await);
    }

    async fn message_delete_bulk(
        &self,
        _ctx: SerenityContext,
        channel: ChannelId,
        messages: Vec<MessageId>,
        _guild: Option<GuildId>,
    ) {
        for message in messages {
            self.report(self.receive_delete(channel, message).await);
        }
    }

    async fn reaction_add(&self, ctx: SerenityContext, reaction: DiscordReaction) {
        let result = self
            .receive_reactions(&ctx.http, reaction.channel_id, reaction.message_id)
            .await;
        self.report(result);
    }

    async fn reaction_remove(&self, ctx: SerenityContext, reaction: DiscordReaction) {
        let result = self
            .receive_reactions(&ctx.http, reaction.channel_id, reaction.message_id)
            .await;
        self.report(result);
    }

    async fn reaction_remove_all(
        &self,
        ctx: SerenityContext,
        channel: ChannelId,
        message: MessageId,
    ) {
        self.report(self.receive_reactions(&ctx.http, channel, message).await);
    }

    async fn reaction_remove_emoji(&self, ctx: SerenityContext, reaction: DiscordReaction) {
        let result = self
            .receive_reactions(&ctx.http, reaction.channel_id, reaction.message_id)
            .await;
        self.report(result);
    }
}

/// Unicode emoji stay as they are, custom emoji become `:name:`.
fn emoji_name(reaction: &ReactionType) -> String {
    match reaction {
        ReactionType::Unicode(emoji) => emoji.clone(),
        ReactionType::Custom { name, .. } => format!(":{}:", name.as_deref().unwrap_or("emoji")),
        _ => "❔".to_owned(),
    }
}

fn mention_names(users: &[User]) -> Vec<(u64, String)> {
    users
        .iter()
        .map(|user| {
            let name = user
                .global_name
                .clone()
                .unwrap_or_else(|| user.name.clone());
            (user.id.get(), name)
        })
        .collect()
}

fn author_of(user: &User, nickname: Option<&str>) -> Author {
    Author {
        display_name: nickname
            .map(str::to_owned)
            .or_else(|| user.global_name.clone()),
        username: user.name.clone(),
        avatar: Some(Avatar::Url(user.face())),
        source: Source::Discord,
    }
}

/// Converts `message` to a core message, with ID 0 for the caller to replace.
async fn to_core(context: &Context, http: &Http, message: &DiscordMessage) -> TaskResult<Message> {
    let nickname = message
        .member
        .as_ref()
        .and_then(|member| member.nick.as_deref());
    let mut parts = vec![];
    let mut attachments = vec![];

    let forward = message
        .message_reference
        .as_ref()
        .filter(|reference| reference.kind == MessageReferenceKind::Forward)
        .and(message.message_snapshots.first());
    let (text, mentions, files) = match forward {
        Some(snapshot) => {
            parts.push("*Forwarded message*".to_owned());
            (&snapshot.content, &snapshot.mentions, &snapshot.attachments)
        }
        None => (&message.content, &message.mentions, &message.attachments),
    };

    for sticker in &message.sticker_items {
        parts.push(format!("*{} sticker*", sticker.name));
        if let Some(url) = sticker.image_url() {
            let file = CreateAttachment::url(http, &url).await?;
            attachments.push(attachment_from(&file.data, &file.filename).await?);
        }
    }
    for file in files {
        if file.size > MAX_DOWNLOAD_BYTES {
            parts.push(format!(
                "*Sent a file that is too large to bridge: {}*",
                file.filename
            ));
        } else {
            attachments.push(attachment_from(&file.download().await?, &file.filename).await?);
        }
    }
    parts.push(content::to_core(text, &mention_names(mentions)));

    let reply = message
        .message_reference
        .as_ref()
        .filter(|reference| reference.kind == MessageReferenceKind::Default)
        .and_then(|reference| reference.message_id);
    let in_reply_to = match reply {
        Some(reply) => {
            context
                .database
                .find_message(&link(context, message.channel_id, reply))
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
        author: author_of(&message.author, nickname),
        content: parts
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        attachments,
        in_reply_to,
        reply_author,
        reactions: vec![],
    })
}

async fn attachment_from(data: &[u8], filename: &str) -> TaskResult<Attachment> {
    let mut file = TempFile::new().await?;
    file.write_all(data).await?;
    file.flush().await?;
    let (filename, spoilered) = match filename.strip_prefix("SPOILER_") {
        Some(name) => (name.to_owned(), true),
        None => (filename.to_owned(), false),
    };
    Ok(Attachment {
        file: Arc::new(file),
        filename,
        spoilered,
    })
}
