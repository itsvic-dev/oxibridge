use markdown::{
    mdast::{self, Node},
    ParseOptions,
};
use regex::Regex;
use teloxide::types::{MessageEntity, MessageEntityKind};
use tracing::debug;

#[derive(Debug, PartialEq, Eq)]
pub struct StringWithEntities(pub Vec<u16>, pub Vec<MessageEntity>);

impl StringWithEntities {
    fn new() -> Self {
        Self(Vec::new(), vec![])
    }

    fn join(&mut self, other: &Self) {
        let offset_entities = other
            .1
            .iter()
            .map(|x| MessageEntity {
                kind: x.kind.clone(),
                offset: x.offset + self.0.len(),
                length: x.length,
            })
            .collect::<Vec<MessageEntity>>();

        self.0.extend(other.0.iter());
        self.1.extend(offset_entities);
    }

    fn join_strings(strings: Vec<Self>) -> Self {
        let mut string = Self::new();

        for other in strings {
            string.join(&other);
        }

        string
    }
}

impl From<String> for StringWithEntities {
    fn from(val: String) -> Self {
        StringWithEntities(val.encode_utf16().collect(), vec![])
    }
}

impl From<&str> for StringWithEntities {
    fn from(val: &str) -> Self {
        StringWithEntities(val.encode_utf16().collect(), vec![])
    }
}

fn nodes_to_entities(nodes: Vec<Node>) -> StringWithEntities {
    StringWithEntities::join_strings(nodes.iter().map(node_to_entities).collect())
}

fn node_to_entities(node: &Node) -> StringWithEntities {
    match node {
        Node::Root(mdast::Root { children, .. })
        | Node::ListItem(mdast::ListItem { children, .. }) => nodes_to_entities(children.clone()),

        Node::Paragraph(mdast::Paragraph { children, .. }) => {
            let mut string = nodes_to_entities(children.clone());
            string.0.push('\n' as u16);
            string.0.push('\n' as u16);
            string
        }

        Node::Text(text) => text.value.clone().into(),

        Node::Strong(strong) => {
            let string = nodes_to_entities(strong.children.clone());

            let entity = MessageEntity::bold(0, string.0.len());
            let entities = [entity].into_iter().chain(string.1).collect();

            StringWithEntities(string.0.clone(), entities)
        }

        Node::Emphasis(em) => {
            let string = nodes_to_entities(em.children.clone());

            let entity = MessageEntity::italic(0, string.0.len());
            let entities = [entity].into_iter().chain(string.1).collect();

            StringWithEntities(string.0.clone(), entities)
        }

        Node::InlineCode(node) => StringWithEntities(
            node.value.encode_utf16().collect(),
            vec![MessageEntity::code(0, node.value.len())],
        ),

        Node::Code(node) => StringWithEntities(
            node.value.encode_utf16().collect(),
            vec![MessageEntity::pre(node.lang.clone(), 0, node.value.len())],
        ),

        Node::Heading(heading) => {
            let string = nodes_to_entities(heading.children.clone());
            let full_heading = ("#".repeat(heading.depth.into()) + " ")
                .encode_utf16()
                .chain(string.0)
                .collect::<Vec<u16>>();

            let entity = MessageEntity::bold(0, full_heading.len());
            let entities = [entity].into_iter().chain(string.1).collect();

            StringWithEntities(full_heading, entities)
        }

        Node::List(list) => {
            let children = list
                .children
                .iter()
                .map(node_to_entities)
                .collect::<Vec<StringWithEntities>>();

            // if unordered, prepend "• " to the children
            // otherwise, prepend "{num}. " to the children
            // append \n in the end
            let children = children
                .iter()
                .enumerate()
                .map(|(i, item)| {
                    let mut str_prefix: StringWithEntities = if list.ordered {
                        format!("{}. ", list.start.unwrap_or(1) + i as u32).into()
                    } else {
                        "• ".into()
                    };

                    str_prefix.join(item);
                    str_prefix.join(&"\n".into());
                    str_prefix
                })
                .collect::<Vec<StringWithEntities>>();

            StringWithEntities::join_strings(children)
        }

        Node::Link(link) => {
            let string = nodes_to_entities(link.children.clone());

            let entity = match link.url.as_str() {
                "x-oxibridge:spoiler" => MessageEntity::spoiler(
                    0,
                    string.0.len(),
                ),
                _ => MessageEntity::text_link(
                    reqwest::Url::parse(&link.url).expect("Failed to parse link's URL"),
                    0,
                    string.0.len(),
                ),
            };

            let entities = [entity].into_iter().chain(string.1).collect();
        
            StringWithEntities(string.0.clone(), entities)
        }

        Node::Blockquote(quote) => {
            let string = nodes_to_entities(quote.children.clone());
            // there is no helper function for blockquotes
            let entity = MessageEntity { kind: MessageEntityKind::Blockquote, offset: 0, length: string.0.len() };
            let entities = [entity].into_iter().chain(string.1).collect();

            StringWithEntities(string.0.clone(), entities)
        }

        Node::Break(_) => "\n".into(),

        _ => StringWithEntities(
            format!("unknown node {node:#?}").encode_utf16().collect(),
            vec![],
        ),
    }
}

pub fn to_string_with_entities(value: &str) -> StringWithEntities {
    // not perfect, but Rust's regex engine doesn't support look-arounds so :/
    let re = Regex::new(r"\|\|(.*)\|\|");
    let parsed_value = match re {
        Ok(re) => {
            re.replace(value, "[$1](x-oxibridge:spoiler)").into_owned()
        }
        Err(e) => {
            debug!("regex creation failed: {e}");
            value.to_owned()
        }
    };

    let node = markdown::to_mdast(&parsed_value, &ParseOptions::default()).unwrap();
    node_to_entities(&node)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bold_italic_correctly() {
        let string = "hello, _**world**_!";

        assert_eq!(
            to_string_with_entities(string),
            StringWithEntities(
                "hello, world!".encode_utf16().collect(),
                vec![MessageEntity::italic(7, 5), MessageEntity::bold(7, 5)]
            )
        );
    }

    #[test]
    fn leaves_newlines_as_is() {
        let string = "hello\nworld";

        assert_eq!(to_string_with_entities(string), string.into());
    }

    #[test]
    fn parses_code_correctly() {
        let string = r#"```rs
println!("hello, world!");
```"#;

        assert_eq!(
            to_string_with_entities(string),
            StringWithEntities(
                r#"println!("hello, world!");"#.encode_utf16().collect(),
                vec![MessageEntity::pre(Some("rs".to_owned()), 0, 26)]
            )
        );
    }

    #[test]
    fn parses_unordered_lists_correctly() {
        let string = r#"- hello
- there"#;

        assert_eq!(
            to_string_with_entities(string),
            StringWithEntities(
                "\u{2022} hello\n\u{2022} there\n".encode_utf16().collect(),
                vec![]
            ),
        );
    }

    #[test]
    fn parses_ordered_lists_correctly() {
        let string = r#"1. hello
2. there"#;

        assert_eq!(
            to_string_with_entities(string),
            StringWithEntities("1. hello\n2. there\n".encode_utf16().collect(), vec![]),
        );
    }

    #[test]
    fn parses_links_correctly() {
        let string = r#"[hello there](https://itsvic.dev)"#;

        assert_eq!(
            to_string_with_entities(string),
            StringWithEntities(
                "hello there".encode_utf16().collect(),
                vec![MessageEntity::text_link(
                    reqwest::Url::parse("https://itsvic.dev").unwrap(),
                    0,
                    11
                )]
            ),
        );
    }
}
