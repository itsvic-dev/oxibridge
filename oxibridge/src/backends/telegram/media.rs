use std::{collections::HashMap, error::Error, sync::Arc};

use async_tempfile::TempFile;
use grammers_client::{
    Client,
    media::{Document, InputMedia, Media, Uploaded},
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

/// What a message's media turns into on the core side.
#[derive(Default)]
pub struct Incoming {
    /// Text that describes the media, shown above the message text.
    pub label: Option<String>,
    pub attachments: Vec<Attachment>,
}

/// Converts the media and service action of `message`. Files are only downloaded if `download` is set.
pub async fn incoming(
    client: &Client,
    message: &TgMessage,
    author_name: &str,
    download: bool,
) -> MediaResult<Incoming> {
    if let Some(action) = message.action() {
        return Ok(Incoming {
            label: action_text(action, author_name),
            attachments: vec![],
        });
    }

    let Some(media) = message.media() else {
        return Ok(Incoming::default());
    };

    let label = match &media {
        Media::Photo(_) | Media::WebPage(_) => None,
        Media::Document(document) if document.raw.voice => Some("*Voice message*".to_owned()),
        Media::Document(document) if document.raw.round => Some("*Video message*".to_owned()),
        Media::Document(_) => None,
        Media::Sticker(sticker) if sticker.is_animated() => {
            return Ok(Incoming {
                label: Some(format!("*{} animated sticker*", sticker.emoji())),
                attachments: vec![],
            });
        }
        Media::Sticker(sticker) => Some(format!("*{} sticker*", sticker.emoji())),
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
    let album = attachments.len() > 1 && attachments.iter().all(is_photo);
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
            items.push(match photo_with_spoiler(attachment, &uploaded) {
                Some(raw) => item.media(raw),
                None => item.photo(uploaded),
            });
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

fn is_photo(attachment: &Attachment) -> bool {
    // Telegram turns GIFs sent as photos into still images
    attachment.is_image() && !attachment.filename.to_ascii_lowercase().ends_with(".gif")
}

async fn upload(client: &Client, attachment: &Attachment) -> MediaResult<Uploaded> {
    let mut file = tokio::fs::File::open(attachment.file.file_path()).await?;
    let size = usize::try_from(file.metadata().await?.len())?;
    Ok(client
        .upload_stream(&mut file, size, attachment.filename.clone())
        .await?)
}

fn photo_with_spoiler(
    attachment: &Attachment,
    uploaded: &Uploaded,
) -> Option<tl::types::InputMediaUploadedPhoto> {
    attachment
        .spoilered
        .then(|| tl::types::InputMediaUploadedPhoto {
            spoiler: true,
            live_photo: false,
            file: uploaded.raw.clone(),
            stickers: None,
            ttl_seconds: None,
            video: None,
        })
}

fn with_file(message: InputMessage, attachment: &Attachment, uploaded: Uploaded) -> InputMessage {
    if !is_photo(attachment) {
        return message.document(uploaded);
    }
    match photo_with_spoiler(attachment, &uploaded) {
        Some(raw) => message.media(raw),
        None => message.photo(uploaded),
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

/// Describes a service message, like someone joining. Returns [`None`] for actions that are not bridged.
fn action_text(action: &tl::enums::MessageAction, author_name: &str) -> Option<String> {
    use tl::enums::MessageAction;

    Some(match action {
        MessageAction::ChatAddUser(added) if added.users.len() == 1 => {
            format!("*{author_name} joined the chat*")
        }
        MessageAction::ChatAddUser(added) => {
            format!("*{author_name} added {} people*", added.users.len())
        }
        MessageAction::ChatJoinedByLink(_) | MessageAction::ChatJoinedByRequest => {
            format!("*{author_name} joined the chat*")
        }
        MessageAction::ChatDeleteUser(_) => format!("*{author_name} left the chat*"),
        MessageAction::PinMessage => format!("*{author_name} pinned a message*"),
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

    use super::{action_text, dice_text, location_url};

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

    #[test]
    fn describes_joins_and_leaves() {
        let joined = tl::enums::MessageAction::ChatJoinedByRequest;
        let left: tl::enums::MessageAction =
            tl::types::MessageActionChatDeleteUser { user_id: 1 }.into();
        assert_eq!(
            action_text(&joined, "Vic").as_deref(),
            Some("*Vic joined the chat*")
        );
        assert_eq!(
            action_text(&left, "Vic").as_deref(),
            Some("*Vic left the chat*")
        );
    }

    #[test]
    fn ignores_other_service_messages() {
        let action: tl::enums::MessageAction = tl::types::MessageActionChatEditTitle {
            title: "x".to_owned(),
        }
        .into();
        assert_eq!(action_text(&action, "Vic"), None);
    }

    #[test]
    fn links_locations_to_a_map() {
        assert_eq!(
            location_url(52.5, 13.4),
            "https://www.google.com/maps/search/?api=1&query=52.5%2C13.4"
        );
    }
}
