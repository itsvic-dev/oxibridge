//! Discord attachment links in message text expire after a day. This swaps them for fresh links.

use serde::{Deserialize, Serialize};

const REFRESH_URL: &str = "https://discord.com/api/v10/attachments/refresh-urls";
const LINK_PREFIXES: [&str; 2] = [
    "https://cdn.discordapp.com/attachments/",
    "https://media.discordapp.net/attachments/",
];

#[derive(Serialize)]
struct Request<'a> {
    attachment_urls: &'a [&'a str],
}

#[derive(Deserialize)]
struct Response {
    refreshed_urls: Vec<Refreshed>,
}

#[derive(Deserialize)]
struct Refreshed {
    original: String,
    refreshed: String,
}

/// Replaces the Discord attachment links in `content` with links that are valid again.
///
/// # Errors
/// Returns an error if Discord rejects the request.
pub async fn refresh_links(
    client: &reqwest::Client,
    token: &str,
    content: &str,
) -> reqwest::Result<String> {
    let links = find_links(content);
    if links.is_empty() {
        return Ok(content.to_owned());
    }
    let response: Response = client
        .post(REFRESH_URL)
        .header("Authorization", token)
        .json(&Request {
            attachment_urls: &links,
        })
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let mut content = content.to_owned();
    for link in response.refreshed_urls {
        content = content.replace(&link.original, &link.refreshed);
    }
    Ok(content)
}

fn find_links(content: &str) -> Vec<&str> {
    let mut links = vec![];
    for (start, _) in content.match_indices("https://") {
        let Some(rest) = content.get(start..) else {
            continue;
        };
        if !LINK_PREFIXES.iter().any(|prefix| rest.starts_with(prefix)) {
            continue;
        }
        let end = rest
            .find(|c: char| c.is_whitespace() || "<>()[]\"'|*`~".contains(c))
            .unwrap_or(rest.len());
        if let Some(link) = rest.get(..end)
            && !links.contains(&link)
        {
            links.push(link);
        }
    }
    links
}

#[cfg(test)]
mod tests {
    use super::find_links;

    #[test]
    fn finds_attachment_links_on_both_hosts() {
        let content = "see https://cdn.discordapp.com/attachments/1/2/a.png?ex=1&is=2 and \
            <https://media.discordapp.net/attachments/3/4/b.mp4>";
        assert_eq!(
            find_links(content),
            vec![
                "https://cdn.discordapp.com/attachments/1/2/a.png?ex=1&is=2",
                "https://media.discordapp.net/attachments/3/4/b.mp4",
            ]
        );
    }

    #[test]
    fn ignores_other_links_and_duplicates() {
        let link = "https://cdn.discordapp.com/attachments/1/2/a.png";
        let content =
            format!("https://example.com/x {link} {link} https://cdn.discordapp.com/emojis/1.png");
        assert_eq!(find_links(&content), vec![link]);
    }
}
