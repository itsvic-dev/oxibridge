use std::time::Duration;

use bytes::{Bytes, BytesMut};
use log::debug;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use reqwest::{RequestBuilder, Response, StatusCode, header};
use serde::de::DeserializeOwned;

use crate::{
    Error,
    model::{
        Bot, BridgedIdentity, CreateIdentity, CreateMessage, Id, Member, Message, Problem,
        UpdateIdentity, Upload, UploadPurpose,
    },
};

pub const DEFAULT_BASE_URL: &str = "https://kove.social";

const MAX_RETRIES: u32 = 3;
const DEFAULT_RETRY_AFTER: Duration = Duration::from_secs(1);
// RFC 3986 unreserved characters, which are safe in a path segment and in `filename*`
const UNRESERVED: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// A client for the REST API, authenticated with one token.
///
/// Requests that hit a rate limit wait for `Retry-After` and try again a few times.
#[derive(Debug, Clone)]
pub struct Client {
    http: reqwest::Client,
    base_url: String,
    token: String,
}

impl Client {
    /// Creates a client for [`DEFAULT_BASE_URL`].
    ///
    /// `token` is a bot token (`bot_…`) or a session token (`sess_…`).
    #[must_use]
    pub fn new(token: &str) -> Self {
        Self::with_base_url(token, DEFAULT_BASE_URL)
    }

    /// Creates a client for the Kove server at `base_url`, like `https://staging.kove.social`.
    #[must_use]
    pub fn with_base_url(token: &str, base_url: &str) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_owned(),
            token: token.to_owned(),
        }
    }

    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub(crate) fn token(&self) -> &str {
        &self.token
    }

    /// The WebSocket URL of the gateway on this server.
    #[must_use]
    pub fn gateway_url(&self) -> String {
        let host = self
            .base_url
            .strip_prefix("https://")
            .map(|host| format!("wss://{host}"))
            .or_else(|| {
                self.base_url
                    .strip_prefix("http://")
                    .map(|host| format!("ws://{host}"))
            })
            .unwrap_or_else(|| self.base_url.clone());
        format!("{host}/gateway?v=1")
    }

    fn url(&self, path: &str) -> String {
        format!("{}/api/v1{path}", self.base_url)
    }

    async fn send<F>(&self, build: F) -> Result<Response, Error>
    where
        F: Fn(&reqwest::Client) -> RequestBuilder + Send,
    {
        let mut retries = 0;
        loop {
            let response = build(&self.http).bearer_auth(&self.token).send().await?;
            let status = response.status();
            if status.is_success() {
                return Ok(response);
            }
            if status == StatusCode::TOO_MANY_REQUESTS && retries < MAX_RETRIES {
                let wait = retry_after(&response).unwrap_or(DEFAULT_RETRY_AFTER);
                debug!(
                    "rate limited on {}, waiting {wait:?}",
                    response.url().path()
                );
                tokio::time::sleep(wait).await;
                retries = retries.saturating_add(1);
                continue;
            }
            return Err(error_from(response).await);
        }
    }

    async fn json<T, F>(&self, build: F) -> Result<T, Error>
    where
        T: DeserializeOwned,
        F: Fn(&reqwest::Client) -> RequestBuilder + Send,
    {
        let body = self.send(build).await?.bytes().await?;
        Ok(serde_json::from_slice(&body)?)
    }

    /// Returns the bot of the token.
    ///
    /// # Errors
    /// Fails with `WRONG_TOKEN_KIND` for a user token, or if the request fails.
    pub async fn current_bot(&self) -> Result<Bot, Error> {
        let url = self.url("/bots/@me");
        self.json(|http| http.get(&url)).await
    }

    /// Creates a bridged identity, or returns the bot's identity with the same platform and remote user ID.
    ///
    /// An existing identity is returned unchanged, so update it with [`Self::update_identity`].
    ///
    /// # Errors
    /// Returns an error if the request fails.
    pub async fn create_identity(
        &self,
        identity: &CreateIdentity,
    ) -> Result<BridgedIdentity, Error> {
        let url = self.url("/bots/@me/identities");
        self.json(|http| http.post(&url).json(identity)).await
    }

    /// Changes a bridged identity of the bot.
    ///
    /// # Errors
    /// Fails with `UNKNOWN_IDENTITY` if the identity is gone, or if the request fails.
    pub async fn update_identity(
        &self,
        identity_id: &Id,
        update: &UpdateIdentity,
    ) -> Result<BridgedIdentity, Error> {
        let url = self.url(&format!("/bots/@me/identities/{}", segment(identity_id)));
        self.json(|http| http.patch(&url).json(update)).await
    }

    /// Returns the profile of any persona in a guild shared with its bot.
    ///
    /// # Errors
    /// Returns an error if the request fails.
    pub async fn identity(&self, identity_id: &Id) -> Result<BridgedIdentity, Error> {
        let url = self.url(&format!("/identities/{}", segment(identity_id)));
        self.json(|http| http.get(&url)).await
    }

    /// Uploads a file, to use its ID once in a message, an avatar or an icon.
    ///
    /// # Errors
    /// Returns [`Error::TooLarge`] for files over [`UploadPurpose::max_size`], or an error if the request fails.
    pub async fn upload(
        &self,
        purpose: UploadPurpose,
        filename: &str,
        data: Bytes,
    ) -> Result<Upload, Error> {
        if data.len() as u64 > purpose.max_size() {
            return Err(Error::TooLarge(purpose.max_size()));
        }
        let url = self.url("/uploads");
        let disposition = content_disposition(filename);
        self.json(|http| {
            http.post(&url)
                .query(&[("purpose", purpose.to_string())])
                .header(header::CONTENT_TYPE, "application/octet-stream")
                .header(header::CONTENT_DISPOSITION, &disposition)
                .body(data.clone())
        })
        .await
    }

    /// Downloads a file from a signed URL, like [`crate::model::Attachment::url`], without the token.
    ///
    /// # Errors
    /// Returns [`Error::TooLarge`] for files over `max_size` bytes, or an error if the request fails.
    pub async fn download(&self, url: &str, max_size: u64) -> Result<Bytes, Error> {
        let mut response = self.http.get(url).send().await?.error_for_status()?;
        if response
            .content_length()
            .is_some_and(|length| length > max_size)
        {
            return Err(Error::TooLarge(max_size));
        }
        let mut data = BytesMut::new();
        while let Some(chunk) = response.chunk().await? {
            if data.len().saturating_add(chunk.len()) as u64 > max_size {
                return Err(Error::TooLarge(max_size));
            }
            data.extend_from_slice(&chunk);
        }
        Ok(data.freeze())
    }

    /// Returns the profile and nickname that an actor has in a guild.
    ///
    /// # Errors
    /// Fails with `UNKNOWN_MEMBER` if the actor is not in the guild, or if the request fails.
    pub async fn member(&self, guild_id: &Id, actor_id: &Id) -> Result<Member, Error> {
        let url = self.url(&format!(
            "/guilds/{}/members/{}",
            segment(guild_id),
            segment(actor_id)
        ));
        self.json(|http| http.get(&url)).await
    }

    /// Returns a message with its reactions.
    ///
    /// # Errors
    /// Returns an error if the request fails.
    pub async fn message(&self, channel_id: &Id, message_id: &Id) -> Result<Message, Error> {
        let url = self.message_url(channel_id, message_id);
        self.json(|http| http.get(&url)).await
    }

    /// Sends a message, as the bridged identity `persona` if it is set.
    ///
    /// # Errors
    /// Returns an error if the request fails.
    pub async fn create_message(
        &self,
        channel_id: &Id,
        message: &CreateMessage,
        persona: Option<&Id>,
    ) -> Result<Message, Error> {
        let url = self.url(&format!("/channels/{}/messages", segment(channel_id)));
        self.json(|http| with_persona(http.post(&url), persona).json(message))
            .await
    }

    /// Replaces the content of an own message, or of a message of the bridged identity `persona`.
    ///
    /// # Errors
    /// Returns an error if the request fails.
    pub async fn edit_message(
        &self,
        channel_id: &Id,
        message_id: &Id,
        content: &str,
        persona: Option<&Id>,
    ) -> Result<Message, Error> {
        let url = self.message_url(channel_id, message_id);
        let body = serde_json::json!({ "content": content });
        self.json(|http| with_persona(http.patch(&url), persona).json(&body))
            .await
    }

    /// Deletes a message, as the bridged identity `persona` if it is set.
    ///
    /// # Errors
    /// Returns an error if the request fails.
    pub async fn delete_message(
        &self,
        channel_id: &Id,
        message_id: &Id,
        persona: Option<&Id>,
    ) -> Result<(), Error> {
        let url = self.message_url(channel_id, message_id);
        self.send(|http| with_persona(http.delete(&url), persona))
            .await?;
        Ok(())
    }

    fn message_url(&self, channel_id: &Id, message_id: &Id) -> String {
        self.url(&format!(
            "/channels/{}/messages/{}",
            segment(channel_id),
            segment(message_id)
        ))
    }
}

fn with_persona(request: RequestBuilder, persona: Option<&Id>) -> RequestBuilder {
    match persona {
        Some(persona) => request.query(&[("as", persona.as_str())]),
        None => request,
    }
}

fn segment(id: &Id) -> String {
    utf8_percent_encode(id.as_str(), UNRESERVED).to_string()
}

fn retry_after(response: &Response) -> Option<Duration> {
    let seconds: f64 = response
        .headers()
        .get(header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .parse()
        .ok()?;
    Duration::try_from_secs_f64(seconds).ok()
}

async fn error_from(response: Response) -> Error {
    let status = response.status().as_u16();
    let body = match response.bytes().await {
        Ok(body) => body,
        Err(e) => return e.into(),
    };
    serde_json::from_slice::<Problem>(&body).map_or(Error::Status(status), |problem| {
        Error::Api(Box::new(problem))
    })
}

fn content_disposition(filename: &str) -> String {
    let fallback: String = filename
        .chars()
        .map(|c| {
            if c == ' ' || (c.is_ascii_graphic() && c != '"' && c != '\\') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let encoded = utf8_percent_encode(filename, UNRESERVED);
    format!("attachment; filename=\"{fallback}\"; filename*=UTF-8''{encoded}")
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    use super::{Client, Error, content_disposition};

    // answers each connection with the next of `responses`, given as (status line and headers, body)
    async fn serve(
        responses: Vec<(&'static str, &'static str)>,
    ) -> std::io::Result<(String, Arc<AtomicUsize>)> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let count = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&count);
        tokio::spawn(async move {
            for (head, body) in responses {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let mut buffer = [0; 4096];
                if stream.read(&mut buffer).await.is_err() {
                    return;
                }
                counter.fetch_add(1, Ordering::SeqCst);
                let reply = format!(
                    "HTTP/1.1 {head}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                if stream.write_all(reply.as_bytes()).await.is_err() {
                    return;
                }
            }
        });
        Ok((format!("http://{address}"), count))
    }

    const BOT: (&str, &str) = (
        "200 OK\r\nContent-Type: application/json",
        r#"{"id":"1","username":"bridge","owner_id":"2","public":false,"suspended":false,"default_permissions":"0"}"#,
    );

    #[tokio::test]
    async fn retries_after_a_rate_limit() -> Result<(), Box<dyn std::error::Error>> {
        let (url, count) =
            serve(vec![("429 Too Many Requests\r\nRetry-After: 0", ""), BOT]).await?;
        let bot = Client::with_base_url("bot_x", &url).current_bot().await?;
        assert_eq!(bot.username, "bridge");
        assert_eq!(count.load(Ordering::SeqCst), 2);
        Ok(())
    }

    #[tokio::test]
    async fn reports_the_problem_code() -> std::io::Result<()> {
        let (url, _) = serve(vec![(
            "403 Forbidden\r\nContent-Type: application/problem+json",
            r#"{"type":"/docs/errors/WRONG_TOKEN_KIND","title":"Wrong token kind","status":403,"code":"WRONG_TOKEN_KIND"}"#,
        )])
        .await?;
        let error = Client::with_base_url("sess_x", &url)
            .current_bot()
            .await
            .err();
        assert_eq!(
            error.as_ref().and_then(Error::code),
            Some("WRONG_TOKEN_KIND")
        );
        Ok(())
    }

    #[tokio::test]
    async fn reports_the_status_of_other_errors() -> std::io::Result<()> {
        let (url, _) = serve(vec![("502 Bad Gateway", "upstream down")]).await?;
        let error = Client::with_base_url("bot_x", &url)
            .current_bot()
            .await
            .err();
        assert_eq!(error.as_ref().and_then(Error::status), Some(502));
        Ok(())
    }

    #[test]
    fn derives_the_gateway_url_from_the_base_url() {
        assert_eq!(
            Client::with_base_url("t", "https://staging.kove.social/").gateway_url(),
            "wss://staging.kove.social/gateway?v=1"
        );
        assert_eq!(
            Client::with_base_url("t", "http://localhost:8080").gateway_url(),
            "ws://localhost:8080/gateway?v=1"
        );
    }

    #[test]
    fn keeps_unicode_file_names_in_uploads() {
        assert_eq!(
            content_disposition("zdjęcie \"1\".png"),
            "attachment; filename=\"zdj_cie _1_.png\"; filename*=UTF-8''zdj%C4%99cie%20%221%22.png"
        );
    }
}
