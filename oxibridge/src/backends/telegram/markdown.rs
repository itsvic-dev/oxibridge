//! Conversion between core message content, which is Discord-flavored markdown, and Telegram text with entities.
//!
//! Telegram entity offsets and lengths count UTF-16 code units.

use grammers_client::tl::{enums::MessageEntity, types};

#[derive(Clone)]
enum Kind {
    Bold,
    Italic,
    Underline,
    Strike,
    Spoiler,
    Code,
    Pre(String),
    Blockquote,
    TextUrl(String),
}

impl Kind {
    fn entity(self, offset: i32, length: i32) -> MessageEntity {
        match self {
            Self::Bold => types::MessageEntityBold { offset, length }.into(),
            Self::Italic => types::MessageEntityItalic { offset, length }.into(),
            Self::Underline => types::MessageEntityUnderline { offset, length }.into(),
            Self::Strike => types::MessageEntityStrike { offset, length }.into(),
            Self::Spoiler => types::MessageEntitySpoiler { offset, length }.into(),
            Self::Code => types::MessageEntityCode { offset, length }.into(),
            Self::Pre(language) => types::MessageEntityPre {
                offset,
                length,
                language,
            }
            .into(),
            Self::Blockquote => types::MessageEntityBlockquote {
                collapsed: false,
                offset,
                length,
            }
            .into(),
            Self::TextUrl(url) => types::MessageEntityTextUrl {
                offset,
                length,
                url,
            }
            .into(),
        }
    }
}

const INLINE_MARKERS: [&str; 6] = ["||", "**", "__", "~~", "*", "_"];

fn marker_kind(marker: &str) -> Kind {
    match marker {
        "||" => Kind::Spoiler,
        "**" => Kind::Bold,
        "__" => Kind::Underline,
        "~~" => Kind::Strike,
        _ => Kind::Italic,
    }
}

fn utf16_len(text: &str) -> i32 {
    i32::try_from(text.encode_utf16().count()).unwrap_or(i32::MAX)
}

#[derive(Default)]
struct Builder {
    text: String,
    length: i32,
    entities: Vec<MessageEntity>,
}

impl Builder {
    fn push(&mut self, text: &str) {
        self.text.push_str(text);
        self.length = self.length.saturating_add(utf16_len(text));
    }

    fn wrap<F: FnOnce(&mut Self)>(&mut self, kind: Kind, content: F) {
        let index = self.entities.len();
        let start = self.length;
        // reserve the slot now, so outer entities come before the entities inside them
        self.entities.push(Kind::Bold.entity(start, 0));
        content(self);
        let length = self.length.saturating_sub(start);
        if length == 0 {
            self.entities.remove(index);
        } else if let Some(slot) = self.entities.get_mut(index) {
            *slot = kind.entity(start, length);
        }
    }
}

/// Builds the Telegram text for a bridged message: `header` in bold on its own line, then `markdown`,
/// then `footer` in italics on its own line.
pub fn bridged(header: &str, markdown: &str, footer: Option<&str>) -> (String, Vec<MessageEntity>) {
    let mut out = Builder::default();
    out.wrap(Kind::Bold, |out| out.push(header));
    out.push("\n");
    parse(markdown, &mut out, true);
    if let Some(footer) = footer {
        out.push("\n");
        out.wrap(Kind::Italic, |out| out.push(footer));
    }
    (out.text, out.entities)
}

/// Parses `input` into `out`. `block_rules` enables line-start syntax: quotes and headings.
fn parse(input: &str, out: &mut Builder, block_rules: bool) {
    let mut rest = input;
    let mut line_start = block_rules;
    while let Some(c) = rest.chars().next() {
        if line_start {
            line_start = false;
            if let Some(after) = block(rest, out) {
                rest = after;
                continue;
            }
        }

        let previous = input
            .get(..input.len().saturating_sub(rest.len()))
            .and_then(|before| before.chars().next_back());
        if let Some(after) = inline(rest, previous, out) {
            rest = after;
            continue;
        }

        out.push(c.encode_utf8(&mut [0; 4]));
        rest = rest.get(c.len_utf8()..).unwrap_or_default();
        line_start = block_rules && c == '\n';
    }
}

fn split_line(text: &str) -> (&str, &str) {
    text.find('\n').map_or((text, ""), |end| text.split_at(end))
}

fn block<'a>(rest: &'a str, out: &mut Builder) -> Option<&'a str> {
    if let Some(quote) = rest.strip_prefix(">>> ") {
        out.wrap(Kind::Blockquote, |out| parse(quote, out, false));
        return Some("");
    }

    if rest.starts_with("> ") {
        let mut lines = vec![];
        let mut remaining = rest;
        while let Some(line) = remaining.strip_prefix("> ") {
            let (line, tail) = split_line(line);
            lines.push(line);
            remaining = tail;
            match tail.strip_prefix('\n') {
                Some(next) if next.starts_with("> ") => remaining = next,
                _ => break,
            }
        }
        out.wrap(Kind::Blockquote, |out| parse(&lines.join("\n"), out, false));
        return Some(remaining);
    }

    for (marker, kind) in [
        ("### ", Kind::Bold),
        ("## ", Kind::Bold),
        ("# ", Kind::Bold),
        ("-# ", Kind::Italic),
    ] {
        if let Some(heading) = rest.strip_prefix(marker) {
            let (line, tail) = split_line(heading);
            out.wrap(kind, |out| parse(line, out, false));
            return Some(tail);
        }
    }
    None
}

fn inline<'a>(rest: &'a str, previous: Option<char>, out: &mut Builder) -> Option<&'a str> {
    if let Some(after) = rest.strip_prefix('\\') {
        let escaped = after.chars().next().filter(char::is_ascii_punctuation)?;
        out.push(escaped.encode_utf8(&mut [0; 4]));
        return after.get(escaped.len_utf8()..);
    }

    if let Some(body) = rest.strip_prefix("```") {
        if let Some((content, after)) = body.split_once("```") {
            let (language, code) = code_block(content);
            out.wrap(Kind::Pre(language), |out| out.push(code));
            return Some(after);
        }
    }

    if let Some(body) = rest.strip_prefix('`') {
        if let Some((code, after)) = body.split_once('`').filter(|(code, _)| !code.is_empty()) {
            out.wrap(Kind::Code, |out| out.push(code));
            return Some(after);
        }
    }

    for marker in INLINE_MARKERS {
        let Some(body) = rest.strip_prefix(marker) else {
            continue;
        };
        let single = marker.len() == 1;
        if single && body.starts_with(char::is_whitespace) {
            continue;
        }
        if marker == "_" && previous.is_some_and(char::is_alphanumeric) {
            continue;
        }
        if let Some(end) = closing(body, marker) {
            let (inner, after) = body.split_at(end);
            out.wrap(marker_kind(marker), |out| parse(inner, out, false));
            return after.get(marker.len()..);
        }
    }

    rest.strip_prefix('[')
        .and_then(|body| masked_link(body, out))
}

/// Finds where `marker` closes in `body`, skipping escapes and code spans.
fn closing(body: &str, marker: &str) -> Option<usize> {
    let single = marker.len() == 1;
    let mut index = 0_usize;
    while let Some(rest) = body.get(index..) {
        let c = rest.chars().next()?;
        let after = rest.get(c.len_utf8()..).unwrap_or_default();

        if c == '\\' {
            let skipped = after.chars().next().map_or(0, char::len_utf8);
            index = index.saturating_add(1).saturating_add(skipped);
            continue;
        }
        if c == '`' {
            if let Some(end) = after.find('`') {
                index = index.saturating_add(end).saturating_add(2);
                continue;
            }
        }
        if let Some(tail) = rest.strip_prefix(marker) {
            // `*` must not close on the first half of `**`
            if single && tail.starts_with(marker) {
                index = index.saturating_add(2);
                continue;
            }
            let before = body.get(..index).and_then(|b| b.chars().next_back());
            let valid = index > 0
                && !(single && before.is_some_and(char::is_whitespace))
                && !(marker == "_" && tail.starts_with(char::is_alphanumeric));
            if valid {
                return Some(index);
            }
        }
        index = index.saturating_add(c.len_utf8());
    }
    None
}

fn code_block(content: &str) -> (String, &str) {
    match content.split_once('\n') {
        Some((first, code))
            if first
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "+-#._".contains(c)) =>
        {
            (first.to_owned(), code.strip_suffix('\n').unwrap_or(code))
        }
        _ => (String::new(), content),
    }
}

fn masked_link<'a>(body: &'a str, out: &mut Builder) -> Option<&'a str> {
    let (text, after) = body.split_once("](")?;
    let (url, after) = after.split_once(')')?;
    let is_web = url.starts_with("https://") || url.starts_with("http://");
    if text.is_empty() || text.contains('\n') || !is_web || url.contains(char::is_whitespace) {
        return None;
    }
    out.wrap(Kind::TextUrl(url.to_owned()), |out| parse(text, out, false));
    Some(after)
}

/// Converts Telegram `text` and its `entities` to core message content.
pub fn to_markdown(text: &str, entities: &[MessageEntity]) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    let clamp = |value: i32| usize::try_from(value).unwrap_or(0).min(units.len());

    let mut spans: Vec<_> = entities
        .iter()
        .filter_map(|entity| {
            let tag = tag(entity)?;
            let start = clamp(tag.offset);
            let end = clamp(tag.offset.saturating_add(tag.length));
            (start < end).then_some((start, end, tag))
        })
        .collect();
    // outer entities first, so nested tags close in the right order
    spans.sort_by_key(|(start, end, _)| (*start, std::cmp::Reverse(*end)));

    // (position, closing tags before opening tags, nesting order, tag)
    let mut insertions: Vec<(usize, u8, usize, String)> = vec![];
    let mut quotes = vec![];
    for (order, (start, end, tag)) in spans.into_iter().enumerate() {
        if tag.quote {
            quotes.push((start, end));
        }
        insertions.push((start, 1, order, tag.open));
        insertions.push((end, 0, usize::MAX.saturating_sub(order), tag.close));
    }
    insertions.sort_by(|a, b| (a.0, a.1, a.2).cmp(&(b.0, b.1, b.2)));

    let mut output: Vec<u16> = Vec::with_capacity(units.len());
    let mut pending = insertions.into_iter().peekable();
    for position in 0..=units.len() {
        while let Some((_, _, _, tag)) = pending.next_if(|(at, ..)| *at == position) {
            output.extend(tag.encode_utf16());
        }
        let Some(&unit) = units.get(position) else {
            break;
        };
        output.push(unit);
        let next = position.saturating_add(1);
        if unit == u16::from(b'\n')
            && quotes
                .iter()
                .any(|&(start, end)| start < next && next < end)
        {
            output.extend("> ".encode_utf16());
        }
    }
    String::from_utf16_lossy(&output)
}

struct Tag {
    offset: i32,
    length: i32,
    open: String,
    close: String,
    quote: bool,
}

fn tag(entity: &MessageEntity) -> Option<Tag> {
    let simple = |offset, length, marker: &str| Tag {
        offset,
        length,
        open: marker.to_owned(),
        close: marker.to_owned(),
        quote: false,
    };
    Some(match entity {
        MessageEntity::Bold(e) => simple(e.offset, e.length, "**"),
        MessageEntity::Italic(e) => simple(e.offset, e.length, "*"),
        MessageEntity::Underline(e) => simple(e.offset, e.length, "__"),
        MessageEntity::Strike(e) => simple(e.offset, e.length, "~~"),
        MessageEntity::Spoiler(e) => simple(e.offset, e.length, "||"),
        MessageEntity::Code(e) => simple(e.offset, e.length, "`"),
        MessageEntity::Blockquote(e) => Tag {
            offset: e.offset,
            length: e.length,
            open: "> ".to_owned(),
            close: String::new(),
            quote: true,
        },
        MessageEntity::Pre(e) => Tag {
            offset: e.offset,
            length: e.length,
            open: format!("```{}\n", e.language),
            close: "\n```".to_owned(),
            quote: false,
        },
        MessageEntity::TextUrl(e) => Tag {
            offset: e.offset,
            length: e.length,
            open: "[".to_owned(),
            close: format!("]({})", e.url),
            quote: false,
        },
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use grammers_client::tl::{enums::MessageEntity, types};

    use super::{Builder, bridged, parse, to_markdown};

    fn parsed(markdown: &str) -> (String, Vec<MessageEntity>) {
        let mut out = Builder::default();
        parse(markdown, &mut out, true);
        (out.text, out.entities)
    }

    fn bold(offset: i32, length: i32) -> MessageEntity {
        types::MessageEntityBold { offset, length }.into()
    }

    fn italic(offset: i32, length: i32) -> MessageEntity {
        types::MessageEntityItalic { offset, length }.into()
    }

    fn underline(offset: i32, length: i32) -> MessageEntity {
        types::MessageEntityUnderline { offset, length }.into()
    }

    fn spoiler(offset: i32, length: i32) -> MessageEntity {
        types::MessageEntitySpoiler { offset, length }.into()
    }

    fn code(offset: i32, length: i32) -> MessageEntity {
        types::MessageEntityCode { offset, length }.into()
    }

    fn pre(offset: i32, length: i32, language: &str) -> MessageEntity {
        types::MessageEntityPre {
            offset,
            length,
            language: language.to_owned(),
        }
        .into()
    }

    fn quote(offset: i32, length: i32) -> MessageEntity {
        types::MessageEntityBlockquote {
            collapsed: false,
            offset,
            length,
        }
        .into()
    }

    fn text_url(offset: i32, length: i32, url: &str) -> MessageEntity {
        types::MessageEntityTextUrl {
            offset,
            length,
            url: url.to_owned(),
        }
        .into()
    }

    #[test]
    fn parses_nested_bold_and_italic() {
        assert_eq!(
            parsed("hello, _**world**_!"),
            ("hello, world!".to_owned(), vec![italic(7, 5), bold(7, 5)])
        );
    }

    #[test]
    fn parses_double_underscores_as_underline() {
        assert_eq!(
            parsed("__under__"),
            ("under".to_owned(), vec![underline(0, 5)])
        );
    }

    #[test]
    fn parses_each_spoiler_separately() {
        assert_eq!(
            parsed("||a|| b ||c||"),
            ("a b c".to_owned(), vec![spoiler(0, 1), spoiler(4, 1)])
        );
    }

    #[test]
    fn leaves_underscores_inside_words_alone() {
        assert_eq!(
            parsed("snake_case_name"),
            ("snake_case_name".to_owned(), vec![])
        );
    }

    #[test]
    fn leaves_spaced_asterisks_alone() {
        assert_eq!(parsed("2 * 3 * 4"), ("2 * 3 * 4".to_owned(), vec![]));
    }

    #[test]
    fn honors_escapes() {
        assert_eq!(
            parsed(r"\*not italic\*"),
            ("*not italic*".to_owned(), vec![])
        );
    }

    #[test]
    fn keeps_markers_inside_code_literal() {
        assert_eq!(parsed("`**a**`"), ("**a**".to_owned(), vec![code(0, 5)]));
    }

    #[test]
    fn parses_code_blocks_with_a_language() {
        assert_eq!(
            parsed("```rs\nfn main() {}\n```"),
            ("fn main() {}".to_owned(), vec![pre(0, 12, "rs")])
        );
    }

    #[test]
    fn parses_quote_lines_into_one_quote() {
        assert_eq!(
            parsed("> a\n> b\nc"),
            ("a\nb\nc".to_owned(), vec![quote(0, 3)])
        );
    }

    #[test]
    fn parses_masked_links() {
        assert_eq!(
            parsed("[site](https://itsvic.dev)"),
            (
                "site".to_owned(),
                vec![text_url(0, 4, "https://itsvic.dev")]
            )
        );
    }

    #[test]
    fn leaves_links_to_other_schemes_as_text() {
        assert_eq!(
            parsed("[x](javascript:alert)"),
            ("[x](javascript:alert)".to_owned(), vec![])
        );
    }

    #[test]
    fn parses_headings_as_bold_lines() {
        assert_eq!(
            parsed("# Title\nbody"),
            ("Title\nbody".to_owned(), vec![bold(0, 5)])
        );
    }

    #[test]
    fn measures_offsets_in_utf16_units() {
        assert_eq!(parsed("🦀 **x**"), ("🦀 x".to_owned(), vec![bold(3, 1)]));
    }

    #[test]
    fn keeps_unclosed_markers_as_text() {
        assert_eq!(parsed("**open"), ("**open".to_owned(), vec![]));
    }

    #[test]
    fn puts_the_header_in_bold_before_the_content() {
        assert_eq!(
            bridged("Vic", "*hi*", None),
            ("Vic\nhi".to_owned(), vec![bold(0, 3), italic(4, 2)])
        );
    }

    #[test]
    fn puts_the_footer_in_italics_without_parsing_it() {
        assert_eq!(
            bridged("Vic", "hi", Some(":a_b_c: 1")),
            (
                "Vic\nhi\n:a_b_c: 1".to_owned(),
                vec![bold(0, 3), italic(7, 9)]
            )
        );
    }

    #[test]
    fn measures_the_header_in_utf16_units() {
        assert_eq!(
            bridged("🦀", "*hi*", None),
            ("🦀\nhi".to_owned(), vec![bold(0, 2), italic(3, 2)])
        );
    }

    #[test]
    fn renders_nested_entities() {
        assert_eq!(
            to_markdown("hello, world!", &[bold(7, 5), italic(7, 5)]),
            "hello, ***world***!"
        );
    }

    #[test]
    fn renders_adjacent_entities() {
        assert_eq!(
            to_markdown("ab", &[bold(0, 1), underline(1, 1)]),
            "**a**__b__"
        );
    }

    #[test]
    fn renders_code_blocks_with_their_language() {
        assert_eq!(
            to_markdown("fn main() {}", &[pre(0, 12, "rs")]),
            "```rs\nfn main() {}\n```"
        );
    }

    #[test]
    fn renders_every_line_of_a_quote() {
        assert_eq!(to_markdown("a\nb\nc", &[quote(0, 3)]), "> a\n> b\nc");
    }

    #[test]
    fn renders_text_links() {
        assert_eq!(
            to_markdown("site", &[text_url(0, 4, "https://itsvic.dev")]),
            "[site](https://itsvic.dev)"
        );
    }

    #[test]
    fn renders_offsets_in_utf16_units() {
        assert_eq!(to_markdown("🦀 x", &[bold(3, 1)]), "🦀 **x**");
    }

    #[test]
    fn ignores_entities_past_the_end() {
        assert_eq!(to_markdown("ab", &[bold(1, 10), spoiler(5, 1)]), "a**b**");
    }

    #[test]
    fn round_trips_through_telegram() {
        let markdown = "**bold** *it* __u__ ~~s~~ ||sp|| `c` [l](https://a.b)";
        let (text, entities) = parsed(markdown);
        assert_eq!(to_markdown(&text, &entities), markdown);
    }
}
