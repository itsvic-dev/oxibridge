use std::error::Error;

use futures::StreamExt;
use irc::{
    client::{Client, Sender},
    proto::{Command, Message as IrcMessage},
};
use log::{debug, warn};
use serde::{Deserialize, Serialize};
use tokio::task::JoinSet;

use crate::{
    backends::{BackendGroup, MessageEvent},
    core::{Author, Message, Source},
    database::Database,
};

// servers cut lines at 512 bytes, including the command and target
const MAX_LINE_BYTES: usize = 400;
// any of these ends an IRC line, so they must never be sent inside one
const LINE_BREAKS: [char; 3] = ['\r', '\n', '\0'];

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub server: String,
    /// Defaults to 6697 with TLS and 6667 without.
    pub port: Option<u16>,
    pub nickname: String,
    #[serde(default = "default_use_tls")]
    pub use_tls: bool,
}

const fn default_use_tls() -> bool {
    true
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GroupConfig {
    channel: String,
}

#[derive(Clone)]
struct Channel {
    group: BackendGroup,
    name: String,
}

pub struct IrcBackend {
    name: String,
    client_config: irc::client::data::Config,
    channels: Vec<Channel>,
    database: Database,
}

impl IrcBackend {
    pub fn new(
        name: &str,
        config: &Config,
        group_configs: &[BackendGroup],
        database: Database,
    ) -> Result<Self, Box<dyn Error>> {
        let channels = group_configs
            .iter()
            .map(|group| {
                let options: GroupConfig = group
                    .config
                    .options()
                    .map_err(|e| format!("backend '{name}' in group '{}': {e}", group.name))?;
                Ok(Channel {
                    group: group.clone(),
                    name: options.channel,
                })
            })
            .collect::<Result<Vec<_>, Box<dyn Error>>>()?;

        let mut joined: Vec<String> = channels.iter().map(|c| c.name.clone()).collect();
        joined.sort_unstable();
        joined.dedup();

        Ok(Self {
            name: name.to_owned(),
            client_config: irc::client::data::Config {
                nickname: Some(config.nickname.clone()),
                server: Some(config.server.clone()),
                port: config.port,
                use_tls: Some(config.use_tls),
                channels: joined,
                ..Default::default()
            },
            channels,
            database,
        })
    }
}

#[async_trait::async_trait]
impl super::Backend for IrcBackend {
    async fn start(&self, tasks: &mut JoinSet<()>) -> Result<(), Box<dyn Error>> {
        let mut client = Client::from_config(self.client_config.clone()).await?;
        client.identify()?;
        let mut stream = client.stream()?;
        let sender = client.sender();
        debug!("IrcBackend '{}' connected", self.name);

        for channel in self.channels.iter().filter(|c| !c.group.config.readonly) {
            let mut rx = channel.group.subscribe();
            let sender = sender.clone();
            let target = channel.name.clone();
            tasks.spawn(async move {
                while let Some(message) = rx.recv().await {
                    send_lines(&sender, &target, &render(&message.event));
                }
            });
        }

        let channels: Vec<_> = self
            .channels
            .iter()
            .filter(|c| !c.group.config.writeonly)
            .cloned()
            .collect();
        let database = self.database.clone();
        let name = self.name.clone();
        tasks.spawn(async move {
            let _client = client;
            while let Some(message) = stream.next().await {
                match message {
                    Ok(message) => receive(&database, &channels, &message).await,
                    Err(e) => {
                        warn!("IrcBackend '{name}' lost its connection: {e}");
                        break;
                    }
                }
            }
        });

        Ok(())
    }
}

fn send_lines(sender: &Sender, target: &str, lines: &[String]) {
    for line in lines {
        if let Err(e) = sender.send(Command::PRIVMSG(target.to_owned(), line.clone())) {
            warn!("failed to send to '{target}': {e}");
        }
    }
}

async fn receive(database: &Database, channels: &[Channel], message: &IrcMessage) {
    let (Command::PRIVMSG(target, text) | Command::NOTICE(target, text)) = &message.command else {
        return;
    };
    let Some(nickname) = message.source_nickname() else {
        return;
    };
    let Some(content) = parse_text(text) else {
        return;
    };

    let author = Author {
        display_name: Some(nickname.to_owned()),
        username: nickname.to_owned(),
        avatar: None,
        source: Source::Irc,
    };

    let mut message = Message {
        id: 0,
        author,
        content,
        attachments: vec![],
        in_reply_to: None,
        reply_author: None,
        reactions: vec![],
    };
    for channel in channels
        .iter()
        .filter(|c| c.name.eq_ignore_ascii_case(target))
    {
        let group = &channel.group;
        message.id = match database
            .create_message(&group.name, &group.backend_name, &message)
            .await
        {
            Ok(id) => id,
            Err(e) => {
                warn!("failed to record message from '{target}': {e}");
                continue;
            }
        };
        group.send(MessageEvent::Create(message.clone())).await;
    }
}

/// Returns [`None`] for CTCP requests other than `ACTION`.
fn parse_text(text: &str) -> Option<String> {
    if let Some(ctcp) = text.strip_prefix('\u{1}') {
        let ctcp = ctcp.strip_suffix('\u{1}').unwrap_or(ctcp);
        let action = ctcp.strip_prefix("ACTION ")?;
        return Some(format!("*{}*", strip_formatting(action)));
    }
    Some(strip_formatting(text))
}

/// Removes mIRC formatting codes, including color numbers.
fn strip_formatting(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\u{2}' | '\u{f}' | '\u{11}' | '\u{16}' | '\u{1d}' | '\u{1e}' | '\u{1f}' => {}
            '\u{3}' => {
                skip_digits(&mut chars);
                if chars.peek() == Some(&',') {
                    let mut lookahead = chars.clone();
                    lookahead.next();
                    if lookahead.peek().is_some_and(char::is_ascii_digit) {
                        chars.next();
                        skip_digits(&mut chars);
                    }
                }
            }
            c => result.push(c),
        }
    }
    result
}

fn skip_digits(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    for _ in 0..2 {
        if chars.next_if(char::is_ascii_digit).is_none() {
            break;
        }
    }
}

/// Each returned line is safe to send as one `PRIVMSG`.
fn render(event: &MessageEvent) -> Vec<String> {
    let (message, edited) = match event {
        MessageEvent::Create(message) => (message, false),
        MessageEvent::Edit(message) => (message, true),
        MessageEvent::Reactions(_) | MessageEvent::Delete(_) => return vec![],
    };

    let mut prefix = message.author.full_name(Some(0));
    if let Some(reply_author) = &message.reply_author {
        prefix = format!("{prefix} -> {}", reply_author.full_name(Some(0)));
    }
    if edited {
        prefix.push_str(" (edited)");
    }
    let prefix = prefix.replace(LINE_BREAKS, " ");

    let mut lines: Vec<String> = message
        .content
        .split(LINE_BREAKS)
        .filter(|line| !line.is_empty())
        .flat_map(|line| split_at_bytes(line, MAX_LINE_BYTES))
        .map(|line| format!("{prefix}: {line}"))
        .collect();
    if !message.attachments.is_empty() {
        lines.push(format!("{prefix}: [Sent an attachment]"));
    }
    lines
}

fn split_at_bytes(text: &str, max: usize) -> Vec<&str> {
    let mut pieces = vec![];
    let mut rest = text;
    while rest.len() > max {
        let mut end = max;
        while !rest.is_char_boundary(end) {
            end = end.saturating_sub(1);
        }
        if end == 0 {
            end = rest.chars().next().map_or(rest.len(), char::len_utf8);
        }
        let (piece, tail) = rest.split_at(end);
        pieces.push(piece);
        rest = tail;
    }
    if !rest.is_empty() {
        pieces.push(rest);
    }
    pieces
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_tempfile::TempFile;

    use super::{parse_text, render, split_at_bytes, strip_formatting};
    use crate::{
        backends::MessageEvent,
        core::{Attachment, Author, Message, PartialAuthor, Source},
    };

    fn message(content: &str) -> Message {
        Message {
            id: 1,
            author: Author {
                display_name: Some("Vic".to_owned()),
                username: "vic".to_owned(),
                avatar: None,
                source: Source::File,
            },
            content: content.to_owned(),
            attachments: vec![],
            in_reply_to: None,
            reply_author: None,
            reactions: vec![],
        }
    }

    #[test]
    fn parses_plain_text() {
        assert_eq!(parse_text("hello").as_deref(), Some("hello"));
    }

    #[test]
    fn parses_actions_as_italics() {
        assert_eq!(
            parse_text("\u{1}ACTION waves\u{1}").as_deref(),
            Some("*waves*")
        );
    }

    #[test]
    fn ignores_other_ctcp_requests() {
        assert_eq!(parse_text("\u{1}VERSION\u{1}"), None);
    }

    #[test]
    fn strips_formatting_codes() {
        assert_eq!(
            strip_formatting("\u{2}bold\u{2} \u{3}04,12red\u{3} \u{3}5x\u{f}"),
            "bold red x"
        );
    }

    #[test]
    fn keeps_a_comma_after_a_color_code() {
        assert_eq!(strip_formatting("\u{3}04,text"), ",text");
    }

    #[test]
    fn prefixes_messages_with_the_author() {
        assert_eq!(
            render(&MessageEvent::Create(message("hi"))),
            vec!["Vic (@file/vic): hi"]
        );
    }

    #[test]
    fn splits_every_kind_of_line_break() {
        assert_eq!(
            render(&MessageEvent::Create(message("a\nQUIT\rb\r\n\nc"))),
            vec![
                "Vic (@file/vic): a",
                "Vic (@file/vic): QUIT",
                "Vic (@file/vic): b",
                "Vic (@file/vic): c",
            ]
        );
    }

    #[test]
    fn removes_line_breaks_from_author_names() {
        let mut from_evil = message("hi");
        from_evil.author.display_name = Some("Evil\r\nQUIT".to_owned());
        assert_eq!(
            render(&MessageEvent::Create(from_evil)),
            vec!["Evil  QUIT (@file/vic): hi"]
        );
    }

    #[test]
    fn splits_long_lines_even_below_one_character() {
        assert_eq!(split_at_bytes("éé", 1), vec!["é", "é"]);
    }

    #[test]
    fn shows_reply_target_and_edits() {
        let mut edit = message("fixed");
        edit.reply_author = Some(PartialAuthor {
            display_name: None,
            username: "bob".to_owned(),
            source: Source::Irc,
        });
        assert_eq!(
            render(&MessageEvent::Edit(edit)),
            vec!["Vic (@file/vic) -> bob (edited): fixed"]
        );
    }

    #[tokio::test]
    async fn mentions_attachments() -> Result<(), async_tempfile::Error> {
        let mut with_file = message("");
        with_file.attachments.push(Attachment {
            file: Arc::new(TempFile::new().await?),
            filename: "a.png".to_owned(),
            spoilered: false,
        });
        assert_eq!(
            render(&MessageEvent::Create(with_file)),
            vec!["Vic (@file/vic): [Sent an attachment]"]
        );
        Ok(())
    }

    #[test]
    fn sends_nothing_for_deletes() {
        assert!(render(&MessageEvent::Delete(1)).is_empty());
    }

    #[test]
    fn sends_nothing_for_reactions() {
        assert!(render(&MessageEvent::Reactions(message("hi"))).is_empty());
    }

    #[test]
    fn splits_long_lines_on_character_boundaries() {
        assert_eq!(split_at_bytes("aébc", 3), vec!["aé", "bc"]);
    }
}
