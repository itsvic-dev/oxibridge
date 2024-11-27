use crate::core;
use color_eyre::eyre::Result;
use serenity::all::{Message, User};

pub async fn to_core_message(message: &Message) -> Result<core::Message> {
    let dsc_author = &message.author;
    let core_author = to_core_author(dsc_author)?;

    Ok(core::Message::new(core_author, message.content.clone(), vec![]).await)
}

pub fn to_core_author(author: &User) -> Result<core::Author> {
    Ok(core::Author {
        username: author.name.clone(),
        display_name: author.global_name.clone(),
        avatar: None, // no need to care rn, tg doesn't need it
    })
}
