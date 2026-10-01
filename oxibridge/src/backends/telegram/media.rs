use std::{error::Error, sync::Arc};

use async_tempfile::TempFile;
use grammers_client::{
    Client,
    media::{Document, Media},
    message::Message as TgMessage,
    tl,
};

use crate::core::Attachment;

// larger files are mentioned instead of bridged, as no other platform takes them anyway
const MAX_DOWNLOAD_BYTES: usize = 50 * 1024 * 1024;

type MediaResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

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
