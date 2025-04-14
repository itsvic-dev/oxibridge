use serde::{Deserialize, Serialize};
use serenity::all::Http;
use color_eyre::Result;

#[derive(Serialize, Deserialize)]
struct RefreshUrlsRequest {
  attachment_urls: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RefreshedUrl {
  pub original: String,
  pub refreshed: String,
}

#[derive(Serialize, Deserialize)]
struct RefreshUrlsResponse {
  attachment_urls: Vec<RefreshedUrl>,
}

// manually fetch refreshed CDN links with reqwest
pub async fn refresh_cdn_links(http: &Http, links: &[&str]) -> Result<Vec<RefreshedUrl>> {
  let request = RefreshUrlsRequest {
    attachment_urls: links.iter().map(|x| x.to_string()).collect(),
  };

  let client = reqwest::Client::new();
  let res = client.post("https://discord.com/api/v9/attachments/refresh-urls")
    .header("Authorization", format!("Bot {}", http.token()))
    .header("Content-Type", "application/json")
    .json(&request)
    .send()
    .await?;

  let body: RefreshUrlsResponse = res.json().await?;

  Ok(body.attachment_urls)
}
