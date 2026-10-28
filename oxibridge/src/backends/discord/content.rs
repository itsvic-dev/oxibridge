//! Text conversions between Discord messages and core messages.

/// Discord's limit for message content, in characters.
pub const MAX_CONTENT_LENGTH: usize = 2000;
const MAX_USERNAME_LENGTH: usize = 80;

/// Replaces Discord's mention, channel, role and custom emoji tags with readable text.
///
/// `users` maps user IDs to usernames, taken from the message's mentions.
/// User mentions become `@dc/username`, which other platforms can send back as a mention.
pub fn to_core(content: &str, users: &[(u64, String)]) -> String {
    let mut result = String::with_capacity(content.len());
    let mut rest = content;
    while let Some(start) = rest.find('<') {
        let (before, tag) = rest.split_at(start);
        result.push_str(before);
        let Some(end) = tag.find('>') else {
            rest = tag;
            break;
        };
        let (inner, after) = (tag.get(1..end).unwrap_or_default(), tag.get(end + 1..));
        match readable_tag(inner, users) {
            Some(text) => {
                result.push_str(&text);
                rest = after.unwrap_or_default();
            }
            None => {
                result.push('<');
                rest = tag.get(1..).unwrap_or_default();
            }
        }
    }
    result.push_str(rest);
    result
}

fn readable_tag(inner: &str, users: &[(u64, String)]) -> Option<String> {
    let is_id = |id: &str| !id.is_empty() && id.chars().all(|c| c.is_ascii_digit());

    if let Some(id) = inner.strip_prefix("@&") {
        return is_id(id).then(|| "@role".to_owned());
    }
    if let Some(id) = inner.strip_prefix('@') {
        let id = id.strip_prefix('!').unwrap_or(id);
        let id: u64 = id.parse().ok()?;
        return Some(users.iter().find(|(user, _)| *user == id).map_or_else(
            || "@unknown user".to_owned(),
            |(_, name)| format!("@dc/{name}"),
        ));
    }
    if let Some(id) = inner.strip_prefix('#') {
        return is_id(id).then(|| "#channel".to_owned());
    }
    let emoji = inner
        .strip_prefix("a:")
        .or_else(|| inner.strip_prefix(':'))?;
    let (name, id) = emoji.split_once(':')?;
    (!name.is_empty() && is_id(id)).then(|| format!(":{name}:"))
}

/// Makes `name` acceptable as a webhook username.
///
/// Discord rejects names that are empty, longer than 80 characters, or contain "discord" or "clyde".
/// Those words get a lookalike letter.
pub fn webhook_username(name: &str) -> String {
    let mut name: String = name.chars().take(MAX_USERNAME_LENGTH).collect();
    for (word, from, to) in [("discord", 'i', 'і'), ("clyde", 'l', 'ӏ')] {
        while let Some(start) = name.to_ascii_lowercase().find(word) {
            let Some(offset) = name
                .get(start..)
                .and_then(|rest| rest.find(|c: char| c.eq_ignore_ascii_case(&from)))
            else {
                break;
            };
            name.replace_range(start + offset..start + offset + 1, &to.to_string());
        }
    }
    if name.trim().is_empty() {
        "unknown".to_owned()
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::{to_core, webhook_username};

    #[test]
    fn names_mentioned_users() {
        let users = vec![(1, "vic".to_owned())];
        assert_eq!(
            to_core("hi <@1> and <@!1>", &users),
            "hi @dc/vic and @dc/vic"
        );
    }

    #[test]
    fn hides_unknown_users_roles_and_channels() {
        assert_eq!(
            to_core("<@2> <@&3> <#4>", &[]),
            "@unknown user @role #channel"
        );
    }

    #[test]
    fn shows_custom_emoji_by_name() {
        assert_eq!(to_core("<:blob:123> <a:party:456>", &[]), ":blob: :party:");
    }

    #[test]
    fn leaves_other_angle_brackets_alone() {
        assert_eq!(
            to_core("a < b, <t:1700000000:R>, <https://x.y>", &[]),
            "a < b, <t:1700000000:R>, <https://x.y>"
        );
    }

    #[test]
    fn disguises_reserved_words_in_usernames() {
        assert_eq!(webhook_username("Discord fan"), "Dіscord fan");
        assert_eq!(webhook_username("CLYDE"), "CӏYDE");
    }

    #[test]
    fn shortens_long_usernames() {
        assert_eq!(webhook_username(&"a".repeat(100)).chars().count(), 80);
    }

    #[test]
    fn replaces_empty_usernames() {
        assert_eq!(webhook_username("  "), "unknown");
    }
}
