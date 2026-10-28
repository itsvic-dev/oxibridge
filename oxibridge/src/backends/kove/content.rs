//! Text conversions between Kove messages and core messages.

use crate::core::Source;

/// Kove's limit for message content, in characters.
pub const MAX_CONTENT_LENGTH: usize = 4000;

const ZERO_WIDTH_SPACE: char = '\u{200b}';

/// Makes core content safe to send to Kove, and turns `@kv/name` into a mention of that user.
///
/// `users` maps lowercase usernames to Kove actor IDs. Mention tags and `@everyone` that came from other
/// platforms are broken up, so that people elsewhere cannot ping Kove users at will.
pub fn to_kove(content: &str, users: &[(String, String)]) -> String {
    let escaped = content
        .replace("<@", &format!("<{ZERO_WIDTH_SPACE}@"))
        .replace("@everyone", &format!("@{ZERO_WIDTH_SPACE}everyone"));
    let mut text = String::with_capacity(escaped.len());
    let mut copied = 0;
    for (range, name) in Source::Kove.mentions(&escaped) {
        let name = name.to_lowercase();
        let Some((_, id)) = users.iter().find(|(user, _)| *user == name) else {
            continue;
        };
        text.push_str(escaped.get(copied..range.start).unwrap_or_default());
        text.push_str("<@");
        text.push_str(id);
        text.push('>');
        copied = range.end;
    }
    text.push_str(escaped.get(copied..).unwrap_or_default());
    text
}

/// Replaces Kove's mention tags with readable text.
///
/// `actors` maps actor IDs to mentions like `@kv/vic` or `@dc/vic`, which other platforms can send back.
pub fn to_core(content: &str, actors: &[(String, String)]) -> String {
    let mut result = String::with_capacity(content.len());
    let mut rest = content;
    while let Some(start) = rest.find("<@") {
        let (before, tag) = rest.split_at(start);
        result.push_str(before);
        let inner = tag.get(2..).unwrap_or_default();
        let (role, inner) = inner
            .strip_prefix('&')
            .map_or((false, inner), |inner| (true, inner));
        let digits = inner
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(inner.len());
        let (id, after) = inner.split_at(digits);
        match after.strip_prefix('>') {
            Some(after) if !id.is_empty() => {
                if role {
                    result.push_str("@role");
                } else {
                    result.push_str(
                        actors
                            .iter()
                            .find(|(actor, _)| actor == id)
                            .map_or("@unknown user", |(_, name)| name),
                    );
                }
                rest = after;
            }
            _ => {
                result.push('<');
                rest = tag.get(1..).unwrap_or_default();
            }
        }
    }
    result.push_str(rest);
    result
}

/// Adds the summary of reactions from other platforms as a last line.
pub fn with_reactions(text: String, summary: Option<&str>) -> String {
    match summary {
        Some(summary) if text.is_empty() => format!("*{summary}*"),
        Some(summary) => format!("{text}\n*{summary}*"),
        None => text,
    }
}

#[cfg(test)]
mod tests {
    use super::{to_core, to_kove, with_reactions};

    fn users() -> Vec<(String, String)> {
        vec![("vic".to_owned(), "123".to_owned())]
    }

    #[test]
    fn mentions_known_kove_users() {
        assert_eq!(to_kove("hi @kv/Vic!", &users()), "hi <@123>!");
    }

    #[test]
    fn leaves_unknown_kove_users_as_text() {
        assert_eq!(to_kove("hi @kv/bob", &users()), "hi @kv/bob");
    }

    #[test]
    fn breaks_up_mention_tags_from_other_platforms() {
        let text = to_kove("<@123> <@&4> @everyone", &[]);
        assert!(!text.contains("<@"));
        assert!(!text.contains("@everyone"));
    }

    #[test]
    fn names_mentioned_actors() {
        let actors = vec![
            ("1".to_owned(), "@kv/vic".to_owned()),
            ("2".to_owned(), "@dc/bob".to_owned()),
        ];
        assert_eq!(to_core("<@1> and <@2>", &actors), "@kv/vic and @dc/bob");
    }

    #[test]
    fn hides_unknown_actors_and_roles() {
        assert_eq!(to_core("<@9> <@&3>", &[]), "@unknown user @role");
    }

    #[test]
    fn leaves_other_angle_brackets_alone() {
        assert_eq!(to_core("a <@ b <@x> <@1", &[]), "a <@ b <@x> <@1");
    }

    #[test]
    fn puts_reactions_on_their_own_line() {
        assert_eq!(with_reactions("hi".to_owned(), Some("👍 2")), "hi\n*👍 2*");
        assert_eq!(with_reactions(String::new(), Some("👍 2")), "*👍 2*");
    }
}
