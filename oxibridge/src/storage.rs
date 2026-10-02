use std::{
    collections::HashMap,
    time::{Duration, SystemTime},
};

use async_tempfile::TempFile;
use color_eyre::Result;
use log::debug;
use s3::{Bucket, Region, creds::Credentials};
use tokio::{io::AsyncReadExt, sync::Mutex};

use crate::config::R2Config;

/// Uploads files to an R2 bucket, so that other services can fetch them by URL.
#[derive(Debug)]
pub struct R2Storage {
    bucket: Box<Bucket>,
    cache: Mutex<HashMap<String, CacheItem>>,
}

#[derive(Debug)]
struct CacheItem {
    url: String,
    expiry_time: SystemTime,
}

const DAY: u32 = 24 * 60 * 60;
// leaves time for the service to fetch the URL before it expires
const EXPIRY_MARGIN: Duration = Duration::from_secs(60);

impl R2Storage {
    /// # Errors
    /// Returns an error if the credentials are not valid.
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

        Ok(Self {
            bucket,
            cache: Mutex::new(HashMap::new()),
        })
    }

    /// Uploads `file` and returns a presigned GET URL to it, valid for 1 day.
    ///
    /// Files are stored by the hash of their content, so each file is uploaded once per day at most.
    ///
    /// # Errors
    /// Returns an error if the file cannot be read or uploaded.
    pub async fn url(&self, file: &TempFile, content_type: &str) -> Result<String> {
        let mut content = Vec::new();
        file.open_ro().await?.read_to_end(&mut content).await?;
        let hash = sha256::digest(&content);

        let mut cache = self.cache.lock().await;
        if let Some(item) = cache.get(&hash)
            && item.expiry_time > SystemTime::now() + EXPIRY_MARGIN
        {
            return Ok(item.url.clone());
        }

        self.bucket
            .put_object_with_content_type(&hash, &content, content_type)
            .await?;
        let url = self.bucket.presign_get(&hash, DAY, None).await?;
        cache.insert(
            hash.clone(),
            CacheItem {
                url: url.clone(),
                expiry_time: SystemTime::now() + Duration::from_secs(DAY.into()),
            },
        );
        debug!("uploaded {hash} to R2");
        Ok(url)
    }
}
