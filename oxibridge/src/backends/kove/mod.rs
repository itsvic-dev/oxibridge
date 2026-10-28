use std::{error::Error, sync::Arc};

use async_tempfile::TempFile;
use bytes::Bytes;
use kove::{
    Client, Dispatch, Event, Gateway, Id, Intent,
    model::{Actor, ActorKind, CreateMessage, Message as KoveMessage, UploadPurpose},
};
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use tokio::{io::AsyncWriteExt, task::JoinSet};

use crate::{
    backends::{BackendGroup, MessageEvent},
    core::{Attachment, Author, Avatar, Message, Source, split_text},
    database::{Database, Link},
};

mod content;
mod personas;

use personas::Personas;

type TaskResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

const MAX_FILES: usize = 10;

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    /// Bot token from the developer portal, starting with `bot_`.
    pub token: String,
    /// Defaults to [`kove::DEFAULT_BASE_URL`].
    pub base_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ChannelId {
    Text(String),
    Number(u64),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GroupConfig {
    channel: ChannelId,
}

#[derive(Clone)]
struct Chat {
    group: BackendGroup,
    channel: Id,
}

#[derive(Clone)]
struct Context {
    name: String,
    client: Client,
    database: Database,
    personas: Arc<Personas>,
}

pub struct KoveBackend {
    name: String,
    client: Client,
    chats: Vec<Chat>,
    database: Database,
}

impl KoveBackend {
    pub fn new(
        name: &str,
        config: &Config,
        group_configs: &[BackendGroup],
        database: Database,
    ) -> Result<Self, Box<dyn Error>> {
        let chats = group_configs
            .iter()
            .map(|group| {
                let options: GroupConfig = group
                    .config
                    .options()
                    .map_err(|e| format!("backend '{name}' in group '{}': {e}", group.name))?;
                let channel = match options.channel {
                    ChannelId::Text(id) => Id(id),
                    ChannelId::Number(id) => Id(id.to_string()),
                };
                Ok(Chat {
                    group: group.clone(),
                    channel,
                })
            })
            .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
        let base_url = config.base_url.as_deref().unwrap_or(kove::DEFAULT_BASE_URL);

        Ok(Self {
            name: name.to_owned(),
            client: Client::with_base_url(&config.token, base_url),
            chats,
            database,
        })
    }
}

#[async_trait::async_trait]
impl super::Backend for KoveBackend {
    async fn start(&self, tasks: &mut JoinSet<()>) -> Result<(), Box<dyn Error>> {
        let bot = self.client.current_bot().await?;
        debug!(
            "KoveBackend '{}' signed in as {} on {}",
            self.name,
            bot.username,
            self.client.base_url()
        );
        let personas =
            Personas::load(&self.name, self.client.clone(), self.database.clone()).await?;
        let context = Context {
            name: self.name.clone(),
            client: self.client.clone(),
            database: self.database.clone(),
            personas: Arc::new(personas),
        };

        for chat in self.chats.iter().filter(|c| !c.group.config.readonly) {
            let mut rx = chat.group.subscribe();
            let context = context.clone();
            let channel = chat.channel.clone();
            tasks.spawn(async move {
                while let Some(message) = rx.recv().await {
                    if let Err(e) = deliver(&context, &channel, &message.event).await {
                        warn!(
                            "KoveBackend '{}' failed to deliver to {channel}: {e}",
                            context.name
                        );
                    }
                }
            });
        }

        let receiver = Receiver {
            chats: self
                .chats
                .iter()
                .filter(|c| !c.group.config.writeonly)
                .cloned()
                .collect(),
            context,
        };
        let mut gateway = Gateway::new(&self.client, vec![Intent::Messages, Intent::Reactions]);
        tasks.spawn(async move {
            loop {
                match gateway.next().await {
                    Ok(dispatch) => receiver.receive(dispatch).await,
                    Err(e) => {
                        warn!("KoveBackend '{}' stopped: {e}", receiver.context.name);
                        break;
                    }
                }
            }
        });
        Ok(())
    }
}

fn link(context: &Context, channel: &Id, message: &Id) -> Link {
    Link {
        backend: context.name.clone(),
        chat: channel.to_string(),
        platform_id: message.to_string(),
    }
}

async fn message_ids(context: &Context, message_id: i64, channel: &Id) -> sqlx::Result<Vec<Id>> {
    Ok(context
        .database
        .links(message_id, &context.name)
        .await?
        .into_iter()
        .filter(|link| link.chat == channel.as_str())
        .map(|link| Id(link.platform_id))
        .collect())
}

async fn deliver(context: &Context, channel: &Id, event: &MessageEvent) -> TaskResult {
    match event {
        MessageEvent::Create(message) => send(context, channel, message).await,
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
            let ids = message_ids(context, message.id, channel).await?;
            let Some(first) = ids.first() else {
                return Ok(());
            };
            let persona = sender(&context.client.message(channel, first).await?.author);
            let text = content::with_reactions(
                outgoing_text(context, message, persona.is_some()).await?,
                message.reaction_summary(&context.name).as_deref(),
            );
            for (id, piece) in ids
                .iter()
                .zip(split_text(&text, content::MAX_CONTENT_LENGTH))
            {
                context
                    .client
                    .edit_message(channel, id, &piece, persona.as_ref())
                    .await?;
            }
            Ok(())
        }
        MessageEvent::Delete(message_id) => {
            let ids = message_ids(context, *message_id, channel).await?;
            // forget the links first, so the delete events Kove sends back find nothing to bridge
            context
                .database
                .remove_links(*message_id, &context.name)
                .await?;
            for id in &ids {
                let persona = match context.client.message(channel, id).await {
                    Ok(message) => sender(&message.author),
                    Err(e) if e.code() == Some("UNKNOWN_MESSAGE") => continue,
                    Err(e) => return Err(e.into()),
                };
                context
                    .client
                    .delete_message(channel, id, persona.as_ref())
                    .await?;
            }
            Ok(())
        }
    }
}

// a message of this bot came from a persona, or from the bot itself when it had no persona
fn sender(author: &Actor) -> Option<Id> {
    (author.kind == ActorKind::Bridged).then(|| author.id.clone())
}

async fn send(context: &Context, channel: &Id, message: &Message) -> TaskResult {
    let persona = match context.personas.persona_for(&message.author).await {
        Ok(persona) => Some(persona),
        Err(e) => {
            warn!(
                "KoveBackend '{}' failed to get a persona for {}, sending as the bot: {e}",
                context.name, message.author.username
            );
            None
        }
    };
    let (uploads, too_large) = uploads(context, message).await?;
    let text = outgoing_text(context, message, persona.is_some()).await? + &too_large;
    let mut pieces = split_text(&text, content::MAX_CONTENT_LENGTH).into_iter();
    let mut chunks = uploads.chunks(MAX_FILES).map(<[String]>::to_vec);
    let count = pieces.len().max(chunks.len());

    let reply_to_id = match message.in_reply_to {
        Some(reply) => message_ids(context, reply, channel)
            .await?
            .into_iter()
            .next(),
        None => None,
    };
    for index in 0..count {
        let request = CreateMessage {
            content: pieces.next().unwrap_or_default(),
            attachments: chunks.next().unwrap_or_default(),
            nonce: Some(format!("ob-{channel}-{}-{index}", message.id)),
            reply_to_id: if index == 0 {
                reply_to_id.clone()
            } else {
                None
            },
            reply_mention: false,
        };
        let sent = context
            .client
            .create_message(channel, &request, persona.as_ref())
            .await?;
        context
            .database
            .add_link(message.id, &link(context, channel, &sent.id))
            .await?;
    }
    Ok(())
}

async fn outgoing_text(
    context: &Context,
    message: &Message,
    persona: bool,
) -> sqlx::Result<String> {
    let mut users = vec![];
    for (_, name) in Source::Kove.mentions(&message.content) {
        if let Some((id, _)) = context.database.find_user(&context.name, name).await? {
            users.push((name.to_lowercase(), id));
        }
    }
    let text = content::to_kove(&message.content, &users);
    if persona {
        Ok(text)
    } else {
        let name = content::to_kove(&message.author.full_name(Some(0)), &[]);
        Ok(format!("**{name}**: {text}"))
    }
}

async fn uploads(context: &Context, message: &Message) -> TaskResult<(Vec<String>, String)> {
    let mut ids = vec![];
    let mut too_large = String::new();
    for attachment in &message.attachments {
        let path = attachment.file.file_path();
        if tokio::fs::metadata(path).await?.len() > UploadPurpose::Attachment.max_size() {
            too_large.push_str("\n*");
            too_large.push_str(&attachment.filename);
            too_large.push_str(" is too large for Kove*");
            continue;
        }
        let data = Bytes::from(tokio::fs::read(path).await?);
        let upload = context
            .client
            .upload(UploadPurpose::Attachment, &attachment.filename, data)
            .await?;
        ids.push(upload.id);
    }
    Ok((ids, too_large))
}

struct Receiver {
    context: Context,
    chats: Vec<Chat>,
}

impl Receiver {
    fn chats_in<'a>(&'a self, channel: &'a Id) -> impl Iterator<Item = &'a Chat> {
        self.chats
            .iter()
            .filter(move |chat| chat.channel == *channel)
    }

    async fn receive(&self, dispatch: Dispatch) {
        // this also drops the messages that this bot sent as personas
        if dispatch.origin_bot_id.is_some() {
            return;
        }
        let result = match &dispatch.event {
            Event::Ready(_) => {
                info!(
                    "KoveBackend '{}' connected to the gateway",
                    self.context.name
                );
                Ok(())
            }
            Event::MessageCreate(message) => self.receive_new(message).await,
            Event::MessageUpdate(message) => self.receive_edit(message).await,
            Event::MessageDelete(event) => self.receive_delete(&event.channel_id, &event.id).await,
            Event::MessageDeleteBulk(event) => {
                let mut result = Ok(());
                for id in &event.ids {
                    result = result.and(self.receive_delete(&event.channel_id, id).await);
                }
                result
            }
            Event::ReactionAdd(event) | Event::ReactionRemove(event) => {
                self.receive_reactions(&event.channel_id, &event.message_id)
                    .await
            }
            Event::ReactionRemoveAll(event) => {
                self.receive_reactions(&event.channel_id, &event.message_id)
                    .await
            }
            _ => Ok(()),
        };
        if let Err(e) = result {
            warn!(
                "KoveBackend '{}' failed to handle an event: {e}",
                self.context.name
            );
        }
    }

    async fn receive_new(&self, message: &KoveMessage) -> TaskResult {
        if message.author.kind != ActorKind::User
            || self.chats_in(&message.channel_id).next().is_none()
        {
            return Ok(());
        }
        let core = to_core(&self.context, message, true).await?;
        if core.content.is_empty() && core.attachments.is_empty() {
            return Ok(());
        }
        let link = link(&self.context, &message.channel_id, &message.id);
        self.context
            .database
            .remember_user(&self.context.name, &core.author, message.author.id.as_str())
            .await?;
        for chat in self.chats_in(&message.channel_id) {
            let group = &chat.group;
            let id = self
                .context
                .database
                .create_message(&group.name, &group.backend_name, &core)
                .await?;
            self.context.database.add_link(id, &link).await?;
            group
                .send(MessageEvent::Create(Message { id, ..core.clone() }))
                .await;
        }
        Ok(())
    }

    async fn receive_edit(&self, message: &KoveMessage) -> TaskResult {
        if message.author.kind != ActorKind::User {
            return Ok(());
        }
        let link = link(&self.context, &message.channel_id, &message.id);
        let database = &self.context.database;
        let Some(id) = database.find_message(&link).await? else {
            return Ok(());
        };
        let fresh = to_core(&self.context, message, false).await?;
        // Kove also sends updates for pins and expired attachments, which keep the content
        if database
            .message(id)
            .await?
            .is_some_and(|stored| stored.content == fresh.content)
        {
            return Ok(());
        }
        database.set_content(id, &fresh.content).await?;
        let core = database
            .message(id)
            .await?
            .unwrap_or(Message { id, ..fresh });
        for chat in self.chats_in(&message.channel_id) {
            chat.group.send(MessageEvent::Edit(core.clone())).await;
        }
        Ok(())
    }

    async fn receive_delete(&self, channel: &Id, message: &Id) -> TaskResult {
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

    async fn receive_reactions(&self, channel: &Id, message: &Id) -> TaskResult {
        if self.chats_in(channel).next().is_none() {
            return Ok(());
        }
        let link = link(&self.context, channel, message);
        let database = &self.context.database;
        let Some(id) = database.find_message(&link).await? else {
            return Ok(());
        };
        let reactions: Vec<_> = self
            .context
            .client
            .message(channel, message)
            .await?
            .reactions
            .unwrap_or_default()
            .into_iter()
            .map(|reaction| (reaction.emoji, i64::from(reaction.count)))
            .collect();
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
}

fn author_of(actor: &Actor) -> Author {
    Author {
        user_id: Some(actor.id.to_string()),
        display_name: actor.display_name.clone(),
        username: actor
            .username
            .clone()
            .unwrap_or_else(|| actor.id.to_string()),
        avatar: actor.avatar_url.clone().map(Avatar::Url),
        source: Source::Kove,
    }
}

// pairs each mentioned actor ID with a mention like `@kv/vic`, or `@dc/vic` for a persona
async fn mention_names(context: &Context, message: &KoveMessage) -> Vec<(String, String)> {
    let mut names = vec![];
    for id in &message.mentions {
        if let Some(name) = context.personas.mention(id).await {
            names.push((id.to_string(), name));
            continue;
        }
        let Some(guild) = &message.guild_id else {
            continue;
        };
        match context.client.member(guild, id).await {
            Ok(member) => {
                if let Some(username) = member.actor.username {
                    names.push((id.to_string(), format!("@kv/{username}")));
                }
            }
            Err(e) => debug!(
                "KoveBackend '{}' could not look up mentioned actor {id}: {e}",
                context.name
            ),
        }
    }
    names
}

async fn to_core(context: &Context, message: &KoveMessage, download: bool) -> TaskResult<Message> {
    let mut parts = vec![];
    let mut attachments = vec![];
    let max_size = UploadPurpose::Attachment.max_size();
    for file in &message.attachments {
        match &file.url {
            None => parts.push(format!("*Sent a file that has expired: {}*", file.filename)),
            Some(_) if file.size > max_size => parts.push(format!(
                "*Sent a file that is too large to bridge: {}*",
                file.filename
            )),
            Some(url) if download => {
                let data = context.client.download(url, max_size).await?;
                attachments.push(attachment_from(&data, &file.filename).await?);
            }
            Some(_) => {}
        }
    }
    parts.push(content::to_core(
        &message.content,
        &mention_names(context, message).await,
    ));

    let in_reply_to = match &message.reply_to_id {
        Some(reply) => {
            context
                .database
                .find_message(&link(context, &message.channel_id, reply))
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
        author: author_of(&message.author),
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
    Ok(Attachment {
        file: Arc::new(file),
        filename: filename.to_owned(),
        spoilered: false,
    })
}
