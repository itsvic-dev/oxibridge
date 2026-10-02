use std::{collections::HashMap, error::Error, sync::Arc};

use async_tempfile::TempFile;
use grammers_client::{
    Client,
    media::{Document, InputMedia, Media, Sticker, Uploaded},
    message::{InputMessage, Message as TgMessage},
    peer::Peer,
    session::types::PeerRef,
    tl,
};
use tokio::sync::Mutex;

use super::markdown::Mentions;

use crate::core::{Attachment, Message};

// larger files are mentioned instead of bridged, as no other platform takes them anyway
const MAX_DOWNLOAD_BYTES: usize = 50 * 1024 * 1024;
// Telegram's limit for media captions, in UTF-16 units
const MAX_CAPTION_LENGTH: usize = 1024;
const MAX_ALBUM_SIZE: usize = 10;

type MediaResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

/// Profile photos of message authors, cached by photo ID.
#[derive(Clone, Default)]
pub struct Avatars(Arc<Mutex<HashMap<i64, Arc<TempFile>>>>);

impl Avatars {
    /// Returns the small profile photo of `peer`, and downloads it if it is not cached.
    pub async fn get(&self, client: &Client, peer: &Peer) -> MediaResult<Option<Arc<TempFile>>> {
        let id = match peer {
            Peer::User(user) => user.photo().map(|photo| photo.photo_id),
            Peer::Group(group) => group.photo().map(|photo| photo.photo_id),
            Peer::Channel(channel) => channel.photo().map(|photo| photo.photo_id),
        };
        let Some(id) = id else {
            return Ok(None);
        };
        if let Some(file) = self.0.lock().await.get(&id) {
            return Ok(Some(Arc::clone(file)));
        }
        let Some(photo) = peer.photo(false).await? else {
            return Ok(None);
        };
        let file = Arc::new(TempFile::new().await?);
        client.download_media(&photo, file.file_path()).await?;
        self.0.lock().await.insert(id, Arc::clone(&file));
        Ok(Some(file))
    }
}

/// Titles and short names of sticker sets, cached by set.
#[derive(Clone, Default)]
pub struct StickerSets(Arc<Mutex<HashMap<String, (String, String)>>>);

impl StickerSets {
    /// Returns a link to the set of `sticker`, titled with the set's name.
    async fn link(&self, client: &Client, sticker: &Sticker) -> Option<String> {
        let set = &sticker.raw_attrs.stickerset;
        let key = match set {
            tl::enums::InputStickerSet::Id(set) => set.id.to_string(),
            tl::enums::InputStickerSet::ShortName(set) => set.short_name.clone(),
            _ => return None,
        };
        let cached = self.0.lock().await.get(&key).cloned();
        let (title, short_name) = match cached {
            Some(found) => found,
            None => {
                let request = tl::functions::messages::GetStickerSet {
                    stickerset: set.clone(),
                    hash: 0,
                };
                let tl::enums::messages::StickerSet::Set(result) =
                    client.invoke(&request).await.ok()?
                else {
                    return None;
                };
                let tl::enums::StickerSet::Set(info) = result.set;
                let found = (info.title, info.short_name);
                self.0.lock().await.insert(key, found.clone());
                found
            }
        };
        Some(format!(
            "[{title}](<https://t.me/addstickers/{short_name}>)"
        ))
    }
}

/// What a message's media turns into on the core side.
#[derive(Default)]
pub struct Incoming {
    /// Text that describes the media, shown above the message text.
    pub label: Option<String>,
    pub attachments: Vec<Attachment>,
}

/// Converts the media of `message`. Files are only downloaded if `download` is set.
pub async fn incoming(
    client: &Client,
    sticker_sets: &StickerSets,
    message: &TgMessage,
    author_name: &str,
    download: bool,
) -> MediaResult<Incoming> {
    let Some(media) = message.media() else {
        return Ok(Incoming::default());
    };

    let label = match &media {
        Media::Photo(_) | Media::WebPage(_) => None,
        Media::Document(document) if document.raw.voice => Some("*Voice message*".to_owned()),
        Media::Document(document) if document.raw.round => Some("*Video message*".to_owned()),
        Media::Document(_) => None,
        Media::Sticker(sticker) => {
            // grammers' is_animated() means GIF; Lottie stickers are TGS files no other platform shows
            let lottie = sticker.document.mime_type() == Some("application/x-tgsticker");
            let kind = if lottie {
                "animated sticker"
            } else {
                "sticker"
            };
            let label = match sticker_sets.link(client, sticker).await {
                Some(set) => format!("*{} {kind} from {set}*", sticker.emoji()),
                None => format!("*{} {kind}*", sticker.emoji()),
            };
            if lottie {
                return Ok(Incoming {
                    label: Some(label),
                    attachments: vec![],
                });
            }
            Some(label)
        }
        Media::Contact(contact) => Some(format!(
            "*Shared a contact*\n{} {}\n{}",
            contact.first_name(),
            contact.last_name(),
            contact.phone_number()
        )),
        Media::Poll(poll) => Some(poll_text(&poll.raw)),
        Media::Geo(geo) => Some(format!(
            "*Shared a location*\n{}",
            location_url(geo.latitue(), geo.longitude())
        )),
        Media::GeoLive(live) => Some(match &live.geo {
            Some(geo) => format!(
                "*Shared a live location*\n{}",
                location_url(geo.latitue(), geo.longitude())
            ),
            None => "*Shared a live location*".to_owned(),
        }),
        Media::Venue(venue) => Some(venue_text(venue)),
        Media::Dice(dice) => Some(format!(
            "*{author_name} rolled a die!*\n{}",
            dice_text(dice.emoji(), dice.value())
        )),
        _ => Some("*Sent an unsupported kind of media*".to_owned()),
    };

    let file = match &media {
        Media::Photo(photo) => Some(("photo.jpg".to_owned(), photo.is_spoiler())),
        Media::Document(document) => Some((document_name(document), document.is_spoiler())),
        Media::Sticker(sticker) => Some((document_name(&sticker.document), false)),
        _ => None,
    };
    let Some((filename, spoilered)) = file.filter(|_| download) else {
        return Ok(Incoming {
            label,
            attachments: vec![],
        });
    };

    if media.size().is_some_and(|size| size > MAX_DOWNLOAD_BYTES) {
        let note = format!("*Sent a file that is too large to bridge: {filename}*");
        return Ok(Incoming {
            label: Some(label.map_or_else(|| note.clone(), |label| format!("{label}\n{note}"))),
            attachments: vec![],
        });
    }

    let file = TempFile::new().await?;
    client.download_media(&media, file.file_path()).await?;
    Ok(Incoming {
        label,
        attachments: vec![Attachment {
            file: Arc::new(file),
            filename,
            spoilered,
        }],
    })
}

/// Sends `message` with its attachments and returns the IDs of the Telegram messages, the captioned one first.
///
/// Telegram cannot mix photos and other files in one album, and limits caption length.
/// In those cases the text is sent first, then each file on its own.
pub async fn send(
    client: &Client,
    target: PeerRef,
    message: &Message,
    reply_to: Option<i32>,
    mentions: &Mentions,
) -> MediaResult<Vec<i32>> {
    let (text, entities) = super::markdown::bridged(
        &message.author.full_name(Some(0)),
        &message.content,
        None,
        mentions,
    );
    let attachments = message.attachments.as_slice();
    let caption_fits = text.encode_utf16().count() <= MAX_CAPTION_LENGTH;
    let album = (2..=MAX_ALBUM_SIZE).contains(&attachments.len())
        && attachments
            .iter()
            .all(|attachment| file_kind(attachment).0.fits_album());
    let text_message = InputMessage::new()
        .text(text.clone())
        .fmt_entities(entities.clone())
        .reply_to(reply_to);

    if let ([attachment], true) = (attachments, caption_fits) {
        let uploaded = upload(client, attachment).await?;
        let sent = client
            .send_message(target, with_file(text_message, attachment, uploaded))
            .await?;
        return Ok(vec![sent.id()]);
    }

    if album && caption_fits {
        let mut items = vec![];
        for (index, attachment) in attachments.iter().enumerate() {
            let uploaded = upload(client, attachment).await?;
            let item = if index == 0 {
                InputMedia::new()
                    .caption(text.clone())
                    .fmt_entities(entities.clone())
                    .reply_to(reply_to)
            } else {
                InputMedia::new()
            };
            items.push(item.media(input_media(attachment, &uploaded)));
        }
        let sent = client.send_album(target, items).await?;
        return Ok(sent.into_iter().flatten().map(|sent| sent.id()).collect());
    }

    let mut ids = vec![client.send_message(target, text_message).await?.id()];
    for attachment in attachments {
        let uploaded = upload(client, attachment).await?;
        let sent = client
            .send_message(target, with_file(InputMessage::new(), attachment, uploaded))
            .await?;
        ids.push(sent.id());
    }
    Ok(ids)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum FileKind {
    Photo,
    Video,
    Audio,
    Other,
}

impl FileKind {
    /// Telegram albums can mix photos and videos.
    fn fits_album(self) -> bool {
        matches!(self, Self::Photo | Self::Video)
    }
}

/// Picks how Telegram shows `attachment`, and its MIME type, by extension.
fn file_kind(attachment: &Attachment) -> (FileKind, &'static str) {
    let extension = std::path::Path::new(&attachment.filename)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("png") => (FileKind::Photo, "image/png"),
        Some("jpg" | "jpeg") => (FileKind::Photo, "image/jpeg"),
        Some("webp") => (FileKind::Photo, "image/webp"),
        Some("mp4" | "m4v") => (FileKind::Video, "video/mp4"),
        Some("mov") => (FileKind::Video, "video/quicktime"),
        Some("mkv") => (FileKind::Video, "video/x-matroska"),
        Some("webm") => (FileKind::Video, "video/webm"),
        Some("mp3") => (FileKind::Audio, "audio/mpeg"),
        Some("m4a") => (FileKind::Audio, "audio/mp4"),
        Some("ogg" | "opus") => (FileKind::Audio, "audio/ogg"),
        Some("flac") => (FileKind::Audio, "audio/flac"),
        Some("wav") => (FileKind::Audio, "audio/wav"),
        // Telegram turns GIFs sent as photos into still images, but plays GIF files
        Some("gif") => (FileKind::Other, "image/gif"),
        _ => (FileKind::Other, "application/octet-stream"),
    }
}

/// Builds the raw media for a photo, video or audio file. Telegram reads the duration and size itself.
fn input_media(attachment: &Attachment, uploaded: &Uploaded) -> tl::enums::InputMedia {
    let (kind, mime_type) = file_kind(attachment);
    if kind == FileKind::Photo {
        return tl::types::InputMediaUploadedPhoto {
            spoiler: attachment.spoilered,
            live_photo: false,
            file: uploaded.raw.clone(),
            stickers: None,
            ttl_seconds: None,
            video: None,
        }
        .into();
    }
    let mut attributes: Vec<tl::enums::DocumentAttribute> = vec![
        tl::types::DocumentAttributeFilename {
            file_name: attachment.filename.clone(),
        }
        .into(),
    ];
    match kind {
        FileKind::Video => attributes.push(
            tl::types::DocumentAttributeVideo {
                round_message: false,
                supports_streaming: true,
                nosound: false,
                duration: 0.0,
                w: 0,
                h: 0,
                preload_prefix_size: None,
                video_start_ts: None,
                video_codec: None,
            }
            .into(),
        ),
        FileKind::Audio => attributes.push(
            tl::types::DocumentAttributeAudio {
                voice: false,
                duration: 0,
                title: None,
                performer: None,
                waveform: None,
            }
            .into(),
        ),
        FileKind::Photo | FileKind::Other => {}
    }
    tl::types::InputMediaUploadedDocument {
        nosound_video: false,
        force_file: false,
        spoiler: attachment.spoilered && kind == FileKind::Video,
        file: uploaded.raw.clone(),
        thumb: None,
        mime_type: mime_type.to_owned(),
        attributes,
        stickers: None,
        video_cover: None,
        video_timestamp: None,
        ttl_seconds: None,
    }
    .into()
}

async fn upload(client: &Client, attachment: &Attachment) -> MediaResult<Uploaded> {
    let mut file = tokio::fs::File::open(attachment.file.file_path()).await?;
    let size = usize::try_from(file.metadata().await?.len())?;
    Ok(client
        .upload_stream(&mut file, size, attachment.filename.clone())
        .await?)
}

fn with_file(message: InputMessage, attachment: &Attachment, uploaded: Uploaded) -> InputMessage {
    match file_kind(attachment).0 {
        FileKind::Other => message.document(uploaded),
        _ => message.media(input_media(attachment, &uploaded)),
    }
}

fn document_name(document: &Document) -> String {
    if let Some(name) = document.name().filter(|name| !name.is_empty()) {
        return name.to_owned();
    }
    let extension = match document.mime_type() {
        Some("image/webp") => "webp",
        Some("video/webm") => "webm",
        Some("video/mp4") => "mp4",
        Some("audio/ogg") => "ogg",
        Some("audio/mpeg") => "mp3",
        Some("image/jpeg") => "jpg",
        Some("image/png") => "png",
        Some("image/gif") => "gif",
        _ => "bin",
    };
    format!("file.{extension}")
}

/// What a service message refers to, beyond its sender.
#[derive(Default)]
pub struct ActionDetails {
    /// Bare user ID of the sender.
    pub sender: Option<i64>,
    /// Names of the users that the action adds or removes, by bare user ID.
    pub names: HashMap<i64, String>,
    /// Content of the pinned message.
    pub pinned: Option<String>,
}

/// Describes a service message, like someone joining. Returns [`None`] for actions that are not bridged.
pub fn action_text(
    action: &tl::enums::MessageAction,
    author_name: &str,
    details: &ActionDetails,
) -> Option<String> {
    use tl::enums::MessageAction;

    let name = |id: &i64| {
        details
            .names
            .get(id)
            .map_or("someone", String::as_str)
            .to_owned()
    };
    Some(match action {
        MessageAction::ChatAddUser(added)
            if added.users.as_slice() == details.sender.as_slice() =>
        {
            format!("*{author_name} joined the chat*")
        }
        MessageAction::ChatAddUser(added) => {
            let names: Vec<_> = added.users.iter().map(name).collect();
            format!("*{author_name} added {}*", names.join(", "))
        }
        MessageAction::ChatJoinedByLink(_) | MessageAction::ChatJoinedByRequest => {
            format!("*{author_name} joined the chat*")
        }
        MessageAction::ChatDeleteUser(removed) if Some(removed.user_id) == details.sender => {
            format!("*{author_name} left the chat*")
        }
        MessageAction::ChatDeleteUser(removed) => {
            format!("*{author_name} removed {}*", name(&removed.user_id))
        }
        MessageAction::PinMessage => {
            match details.pinned.as_deref().filter(|text| !text.is_empty()) {
                Some(text) => format!("*{author_name} pinned a message:*\n{text}"),
                None => format!("*{author_name} pinned a message*"),
            }
        }
        _ => return None,
    })
}

fn poll_text(poll: &tl::types::Poll) -> String {
    let tl::enums::TextWithEntities::Entities(question) = &poll.question;
    let mut text = format!("*Poll:* {}", question.text);
    for answer in &poll.answers {
        let tl::enums::PollAnswer::Answer(answer) = answer else {
            continue;
        };
        let tl::enums::TextWithEntities::Entities(answer) = &answer.text;
        text.push_str("\n• ");
        text.push_str(&answer.text);
    }
    text
}

fn venue_text(venue: &grammers_client::media::Venue) -> String {
    let mut text = format!(
        "*Shared a venue*\n**{}**\n{}",
        venue.raw_venue.title, venue.raw_venue.address
    );
    if let Some(geo) = &venue.geo {
        text.push('\n');
        text.push_str(&location_url(geo.latitue(), geo.longitude()));
    }
    text
}

fn location_url(latitude: f64, longitude: f64) -> String {
    format!("https://www.google.com/maps/search/?api=1&query={latitude}%2C{longitude}")
}

/// Shows the result of a Telegram dice roll. Telegram documents only the dice and darts values.
fn dice_text(emoji: &str, value: i32) -> String {
    match emoji.trim_end_matches('\u{fe0f}') {
        "🎲" => match value {
            1 => "⚀",
            2 => "⚁",
            3 => "⚂",
            4 => "⚃",
            5 => "⚄",
            6 => "⚅",
            _ => "🎲",
        }
        .to_owned(),
        "🎯" => match value {
            1 => "🎯❌ Miss!",
            2 => "🎯 Outer ring",
            3 => "🎯 Middle ring",
            4 => "🎯 Inner ring",
            5 => "🎯 Close to the center",
            6 => "🎯🎯🎯 Bullseye!",
            _ => "🎯",
        }
        .to_owned(),
        "🎳" => match value {
            1 => "🎳❌ Gutter!",
            2 => "🎳 1 pin down",
            3 => "🎳 3 pins down",
            4 => "🎳 4 pins down",
            5 => "🎳 5 pins down",
            6 => "🎳🎳 Strike!",
            _ => "🎳",
        }
        .to_owned(),
        "🏀" => if value >= 4 {
            "🏀🔥 Score!"
        } else {
            "🏀❌ Miss!"
        }
        .to_owned(),
        "⚽" => if value >= 3 {
            "⚽🥅 Goal!"
        } else {
            "⚽❌ Miss!"
        }
        .to_owned(),
        "🎰" => slot_text(value),
        emoji => format!("{emoji} {value}"),
    }
}

fn slot_text(value: i32) -> String {
    const SYMBOLS: [&str; 4] = ["⬛", "🍇", "🍋", "7️⃣"];
    let index = value.saturating_sub(1);
    let reels: Vec<i32> = (0..3).map(|reel| (index >> (reel * 2)) & 3).collect();
    let shown: Vec<&str> = reels
        .iter()
        .map(|&reel| {
            usize::try_from(reel)
                .ok()
                .and_then(|reel| SYMBOLS.get(reel).copied())
                .unwrap_or("🎰")
        })
        .collect();
    let text = format!("[{}]", shown.join(" "));
    if reels.iter().all(|&reel| reel == 3) {
        format!("{text} 🎉 JACKPOT! 🎉")
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use grammers_client::tl;

    use super::{ActionDetails, FileKind, action_text, dice_text, file_kind, location_url};
    use crate::core::Attachment;

    async fn attachment(filename: &str) -> Result<Attachment, async_tempfile::Error> {
        Ok(Attachment {
            file: std::sync::Arc::new(async_tempfile::TempFile::new().await?),
            filename: filename.to_owned(),
            spoilered: false,
        })
    }

    #[tokio::test]
    async fn picks_the_kind_of_file_by_extension() -> Result<(), async_tempfile::Error> {
        assert_eq!(file_kind(&attachment("a.JPG").await?).0, FileKind::Photo);
        assert_eq!(file_kind(&attachment("a.mov").await?).0, FileKind::Video);
        assert_eq!(file_kind(&attachment("a.flac").await?).0, FileKind::Audio);
        assert_eq!(file_kind(&attachment("a.gif").await?).0, FileKind::Other);
        assert_eq!(file_kind(&attachment("a").await?).0, FileKind::Other);
        Ok(())
    }

    #[test]
    fn shows_dice_faces() {
        assert_eq!(dice_text("🎲", 3), "⚂");
    }

    #[test]
    fn decodes_slot_machine_reels() {
        assert_eq!(dice_text("🎰", 1), "[⬛ ⬛ ⬛]");
        assert_eq!(dice_text("🎰", 2), "[🍇 ⬛ ⬛]");
    }

    #[test]
    fn celebrates_a_jackpot() {
        assert_eq!(dice_text("🎰", 64), "[7️⃣ 7️⃣ 7️⃣] 🎉 JACKPOT! 🎉");
    }

    #[test]
    fn shows_unknown_dice_with_their_value() {
        assert_eq!(dice_text("🃏", 2), "🃏 2");
    }

    fn details(sender: i64, names: &[(i64, &str)]) -> ActionDetails {
        ActionDetails {
            sender: Some(sender),
            names: names
                .iter()
                .map(|&(id, name)| (id, name.to_owned()))
                .collect(),
            pinned: None,
        }
    }

    #[test]
    fn describes_joins_and_leaves() {
        let joined: tl::enums::MessageAction =
            tl::types::MessageActionChatAddUser { users: vec![1] }.into();
        let left: tl::enums::MessageAction =
            tl::types::MessageActionChatDeleteUser { user_id: 1 }.into();
        assert_eq!(
            action_text(&joined, "Vic", &details(1, &[])).as_deref(),
            Some("*Vic joined the chat*")
        );
        assert_eq!(
            action_text(&left, "Vic", &details(1, &[])).as_deref(),
            Some("*Vic left the chat*")
        );
    }

    #[test]
    fn names_the_users_others_add_or_remove() {
        let added: tl::enums::MessageAction =
            tl::types::MessageActionChatAddUser { users: vec![2, 3] }.into();
        let removed: tl::enums::MessageAction =
            tl::types::MessageActionChatDeleteUser { user_id: 2 }.into();
        let details = details(1, &[(2, "Bob")]);
        assert_eq!(
            action_text(&added, "Vic", &details).as_deref(),
            Some("*Vic added Bob, someone*")
        );
        assert_eq!(
            action_text(&removed, "Vic", &details).as_deref(),
            Some("*Vic removed Bob*")
        );
    }

    #[test]
    fn shows_the_pinned_message() {
        let details = ActionDetails {
            pinned: Some("hello".to_owned()),
            ..ActionDetails::default()
        };
        assert_eq!(
            action_text(&tl::enums::MessageAction::PinMessage, "Vic", &details).as_deref(),
            Some("*Vic pinned a message:*\nhello")
        );
    }

    #[test]
    fn ignores_other_service_messages() {
        let action: tl::enums::MessageAction = tl::types::MessageActionChatEditTitle {
            title: "x".to_owned(),
        }
        .into();
        assert_eq!(action_text(&action, "Vic", &ActionDetails::default()), None);
    }

    #[test]
    fn links_locations_to_a_map() {
        assert_eq!(
            location_url(52.5, 13.4),
            "https://www.google.com/maps/search/?api=1&query=52.5%2C13.4"
        );
    }
}
