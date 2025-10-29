use std::{
    collections::HashMap,
    time::{Duration, SystemTime},
};

use async_tempfile::TempFile;
use color_eyre::Result;
use log::debug;
use s3::{creds::Credentials, Bucket, Region};
use tokio::io::AsyncReadExt;

use crate::config::R2Config;

#[derive(Debug)]
pub struct R2Storage {
    bucket: Box<Bucket>,
    cache: HashMap<String, CacheItem>,
}

#[derive(Debug)]
struct CacheItem {
    pub url: String,
    pub expiry_time: SystemTime,
}

const DAY: u32 = 24 * 60 * 60;

impl R2Storage {
    pub fn new(config: &R2Config) -> Result<Self> {
        let bucket = Bucket::new(
            &config.bucket_name,
            Region::R2 {
                account_id: config.account_id.clone(),
            },
            Credentials::new(
                Some(&config.access_key),
                Some(&config.secret_key),
                None,
                None,
                None,
            )?,
        )?
        .with_path_style();

        Ok(R2Storage {
            bucket,
            cache: HashMap::new(),
        })
    }

    /// Gets a URL to this file in R2-backed storage.
    ///
    /// The URL is a presigned GET URL which will expire after 1 day.
    pub async fn get_url(&mut self, file: &TempFile) -> Result<String> {
        // read file
        let mut content = Vec::new();
        // Ensure `file` is declared as mutable
        let mut file = file.open_ro().await?;

        file.read_to_end(&mut content).await?;

        let hash = sha256::digest(&content);

        // check if file is in cache. subtracting 10s from expiry time to account for possible latency between the cache hit and Discord pulling it
        if let Some(cache_item) = self.cache.get(&hash) {
            if cache_item.expiry_time - Duration::from_secs(10) >= SystemTime::now() {
                debug!("cache hit for file {file:?}");
                return Ok(cache_item.url.clone());
            }
        }

        // upload the file to S3 and get new presigned URL
        self.bucket.put_object(&hash, &content).await?;
        let url = self.bucket.presign_get(&hash, DAY, None).await?;
        self.cache.insert(
            hash,
            CacheItem {
                url: url.clone(),
                expiry_time: SystemTime::now() + Duration::from_secs(DAY.into()),
            },
        );

        debug!("uploaded file {file:?}");

        Ok(url)
    }
}
