use chrono::{DateTime, Utc};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::addon_schema::{
    AddonHealth, AddonManifest, AnimeMetadata, CatalogRequest, CatalogResponse, SearchRequest,
    VideoSourceRequest, VideoSourceResponse,
};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AddonSource {
    BuiltIn,
    User,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
pub struct AddonRegistration {
    pub id: Uuid,
    pub base_url: String,
    pub enabled: bool,
    pub source: AddonSource,
    pub deletable: bool,
    pub manifest: Option<AddonManifest>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
pub struct MetadataCacheEntry {
    pub id: Uuid,
    pub addon_id: Uuid,
    pub cache_key: String,
    pub payload_json: String,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
pub struct RegisterAddonRequest {
    pub base_url: String,
}

#[derive(Clone)]
pub struct RemoteAddonClient {
    http: reqwest::Client,
}

impl Default for RemoteAddonClient {
    fn default() -> Self {
        Self::new()
    }
}

impl RemoteAddonClient {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::new(),
        }
    }

    pub async fn health(&self, base_url: &str) -> Result<AddonHealth, AddonClientError> {
        self.get_json(base_url, "health").await
    }

    pub async fn manifest(&self, base_url: &str) -> Result<AddonManifest, AddonClientError> {
        self.get_json(base_url, "manifest").await
    }

    pub async fn catalog(
        &self,
        base_url: &str,
        request: &CatalogRequest,
    ) -> Result<CatalogResponse, AddonClientError> {
        self.post_json(base_url, "catalog", request).await
    }

    pub async fn search(
        &self,
        base_url: &str,
        request: &SearchRequest,
    ) -> Result<CatalogResponse, AddonClientError> {
        self.post_json(base_url, "search", request).await
    }

    pub async fn anime_meta(
        &self,
        base_url: &str,
        anime_id: &str,
    ) -> Result<AnimeMetadata, AddonClientError> {
        let path = format!("anime/{anime_id}");
        self.get_json(base_url, &path).await
    }

    pub async fn manga_meta(
        &self,
        base_url: &str,
        manga_id: &str,
    ) -> Result<AnimeMetadata, AddonClientError> {
        let path = format!("manga/{manga_id}");
        self.get_json(base_url, &path).await
    }

    pub async fn video_sources(
        &self,
        base_url: &str,
        request: &VideoSourceRequest,
    ) -> Result<VideoSourceResponse, AddonClientError> {
        self.post_json(base_url, "videos", request).await
    }

    async fn get_json<T>(&self, base_url: &str, path: &str) -> Result<T, AddonClientError>
    where
        T: serde::de::DeserializeOwned,
    {
        let url = addon_url(base_url, path)?;
        let response = self.http.get(url).send().await?.error_for_status()?;
        Ok(response.json().await?)
    }

    async fn post_json<T, B>(
        &self,
        base_url: &str,
        path: &str,
        body: &B,
    ) -> Result<T, AddonClientError>
    where
        T: serde::de::DeserializeOwned,
        B: Serialize + ?Sized,
    {
        let url = addon_url(base_url, path)?;
        let response = self
            .http
            .post(url)
            .json(body)
            .send()
            .await?
            .error_for_status()?;
        Ok(response.json().await?)
    }
}

fn addon_url(base_url: &str, path: &str) -> Result<Url, AddonClientError> {
    let mut base = Url::parse(base_url).map_err(AddonClientError::InvalidUrl)?;
    if !base.path().ends_with('/') {
        base.set_path(&format!("{}/", base.path()));
    }
    base.join(path).map_err(AddonClientError::InvalidUrl)
}

#[derive(Debug, Error)]
pub enum AddonClientError {
    #[error("invalid addon url: {0}")]
    InvalidUrl(url::ParseError),
    #[error("addon request failed: {0}")]
    Request(#[from] reqwest::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addon_url_preserves_nested_base_path() {
        let url = addon_url("https://addons.example/typenx", "manifest").unwrap();
        assert_eq!(url.as_str(), "https://addons.example/typenx/manifest");
    }
}
