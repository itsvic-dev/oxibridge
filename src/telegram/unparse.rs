// Port of the markdown.unparse function from Pyrogram.
// https://github.com/pyrogram/pyrogram/blob/master/pyrogram/parser/markdown.py#L119

use teloxide::types::{MessageEntity, MessageEntityKind};

use super::entities::StringWithEntities;

fn unparse_entities_impl(text: StringWithEntities) -> String {
  let mut entities_offsets: Vec<(&str, usize)> = vec![];

  for entity in text.1 {
    let start = entity.offset;
    let end = start + entity.length;

    let tags = match entity.kind {
      MessageEntityKind::Bold => Some(("**", "**")),
      MessageEntityKind::Italic => Some(("_", "_")),
      MessageEntityKind::Strikethrough => Some(("~~", "~~")),
      MessageEntityKind::Underline => Some(("__", "__")),
      MessageEntityKind::Code => Some(("``", "``")),
      MessageEntityKind::Pre { language } => match language {
        // TODO: consider language
        Some(_) => Some(("```", "```")),
        None => Some(("```", "```"))
      },
      // FIXME: won't work correctly with newlines. but bleh
      MessageEntityKind::Blockquote => Some(("> ", "")),
      MessageEntityKind::Spoiler => Some(("||", "||")),
      // TODO: text links, text mentions
      _ => None,
    };

    if let Some((start_tag, end_tag)) = tags {
      entities_offsets.push((start_tag, start));
      if !end_tag.is_empty() {
        entities_offsets.push((end_tag, end));
      }
    }
  }

  let mut entities_enumerated: Vec<_> = entities_offsets.iter().enumerate().collect();
  entities_enumerated.sort_by_key(|x| (x.1.1, x.0));
  entities_enumerated.reverse();
  let entities_offsets: Vec<_> = entities_enumerated.iter().map(|x| x.1).collect();

  let mut text = text.0;

  for (entity, offset) in entities_offsets {
    let mut entity: Vec<u16> = entity.encode_utf16().collect();
    let offset = *offset;
    let mut text_left: Vec<u16> = text[..offset].to_vec();
    let mut text_right: Vec<u16> = text[offset..].to_vec();
    text_left.append(&mut entity);
    text_left.append(&mut text_right);

    text = text_left;
  }

  String::from_utf16_lossy(&text)
}

pub fn unparse_entities(text: &str, entities: Vec<MessageEntity>) -> String {
  unparse_entities_impl(StringWithEntities(text.encode_utf16().collect(), entities))
}

#[cfg(test)]
mod tests {
  use teloxide::types::MessageEntity;
  use crate::telegram::entities::StringWithEntities;
  use super::unparse_entities_impl;

  fn string_with_entities(text: &str, entities: Vec<MessageEntity>) -> StringWithEntities {
    StringWithEntities(text.encode_utf16().collect(), entities)
  }

  #[test]
  fn unparses_bold_text() {
    let string = string_with_entities("hello, world!", vec![
      MessageEntity::bold(7, 5)
    ]);

    assert_eq!(
      unparse_entities_impl(string),
      "hello, **world**!".to_owned()
    );
  }

  #[test]
  fn unparses_italic_text() {
    let string = string_with_entities("hello, world!", vec![
      MessageEntity::italic(7, 5)
    ]);

    assert_eq!(
      unparse_entities_impl(string),
      "hello, _world_!".to_owned()
    );
  }
}
