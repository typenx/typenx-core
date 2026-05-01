use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    auth::{AuthProvider, ProviderIdentity},
    library::{AnimeListEntry, ProviderListSync, WatchStatus},
    security::random_url_token,
};

#[async_trait]
pub trait AnimeProviderClient: Send + Sync {
    fn provider(&self) -> AuthProvider;
    fn authorization_url(&self, state: &str, pkce_challenge: Option<&str>) -> String;
    async fn exchange_code(
        &self,
        code: &str,
        pkce_verifier: Option<&str>,
    ) -> Result<ProviderIdentity, ProviderError>;
    async fn sync_list(
        &self,
        identity: &ProviderIdentity,
    ) -> Result<ProviderListSync, ProviderError>;
}

#[derive(Clone, Debug)]
pub struct OAuthProviderConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

#[derive(Clone, Debug)]
pub struct AniListClient {
    http: reqwest::Client,
    config: OAuthProviderConfig,
    authorize_url: String,
    token_url: String,
    graphql_url: String,
}

impl AniListClient {
    pub fn new(config: OAuthProviderConfig) -> Self {
        Self {
            http: reqwest::Client::new(),
            config,
            authorize_url: "https://anilist.co/api/v2/oauth/authorize".to_owned(),
            token_url: "https://anilist.co/api/v2/oauth/token".to_owned(),
            graphql_url: "https://graphql.anilist.co".to_owned(),
        }
    }

    pub fn with_endpoints(
        config: OAuthProviderConfig,
        authorize_url: String,
        token_url: String,
        graphql_url: String,
    ) -> Self {
        Self {
            http: reqwest::Client::new(),
            config,
            authorize_url,
            token_url,
            graphql_url,
        }
    }
}

#[async_trait]
impl AnimeProviderClient for AniListClient {
    fn provider(&self) -> AuthProvider {
        AuthProvider::AniList
    }

    fn authorization_url(&self, state: &str, _pkce_challenge: Option<&str>) -> String {
        let mut url =
            Url::parse(&self.authorize_url).expect("static AniList authorize URL is valid");
        url.query_pairs_mut()
            .append_pair("client_id", &self.config.client_id)
            .append_pair("redirect_uri", &self.config.redirect_uri)
            .append_pair("response_type", "code")
            .append_pair("state", state);
        url.to_string()
    }

    async fn exchange_code(
        &self,
        code: &str,
        _pkce_verifier: Option<&str>,
    ) -> Result<ProviderIdentity, ProviderError> {
        let token = self
            .http
            .post(&self.token_url)
            .json(&AniListTokenRequest {
                grant_type: "authorization_code",
                client_id: &self.config.client_id,
                client_secret: &self.config.client_secret,
                redirect_uri: &self.config.redirect_uri,
                code,
            })
            .send()
            .await?
            .error_for_status()?
            .json::<OAuthTokenResponse>()
            .await?;

        let viewer = self.viewer(&token.access_token).await?;
        Ok(ProviderIdentity {
            provider: AuthProvider::AniList,
            provider_user_id: viewer.id.to_string(),
            username: viewer.name,
            avatar_url: viewer.avatar.and_then(|avatar| avatar.large),
            access_token: token.access_token,
            refresh_token: token.refresh_token,
            expires_at: token
                .expires_in
                .map(|seconds| Utc::now() + Duration::seconds(seconds)),
        })
    }

    async fn sync_list(
        &self,
        identity: &ProviderIdentity,
    ) -> Result<ProviderListSync, ProviderError> {
        let response = self
            .graphql::<AniListMediaListCollectionData>(
                &identity.access_token,
                ANILIST_LIST_QUERY,
                serde_json::json!({ "userId": identity.provider_user_id.parse::<i64>().unwrap_or_default() }),
            )
            .await?;
        let now = Utc::now();
        let entries = response
            .media_list_collection
            .lists
            .into_iter()
            .flat_map(|list| list.entries)
            .filter_map(|entry| entry.into_library_entry(identity, now))
            .collect();
        Ok(ProviderListSync {
            provider: AuthProvider::AniList,
            entries,
            synced_at: now,
        })
    }
}

impl AniListClient {
    async fn viewer(&self, access_token: &str) -> Result<AniListViewer, ProviderError> {
        let response = self
            .graphql::<AniListViewerData>(access_token, ANILIST_VIEWER_QUERY, serde_json::json!({}))
            .await?;
        Ok(response.viewer)
    }

    async fn graphql<T: serde::de::DeserializeOwned>(
        &self,
        access_token: &str,
        query: &str,
        variables: serde_json::Value,
    ) -> Result<T, ProviderError> {
        let response = self
            .http
            .post(&self.graphql_url)
            .bearer_auth(access_token)
            .json(&GraphQlRequest { query, variables })
            .send()
            .await?
            .error_for_status()?
            .json::<GraphQlResponse<T>>()
            .await?;
        response.data.ok_or_else(|| {
            ProviderError::InvalidData(
                response
                    .errors
                    .unwrap_or_default()
                    .into_iter()
                    .map(|error| error.message)
                    .collect::<Vec<_>>()
                    .join(", "),
            )
        })
    }
}

#[derive(Clone, Debug)]
pub struct MyAnimeListClient {
    http: reqwest::Client,
    config: OAuthProviderConfig,
    authorize_url: String,
    token_url: String,
    api_url: String,
}

impl MyAnimeListClient {
    pub fn new(config: OAuthProviderConfig) -> Self {
        Self {
            http: reqwest::Client::new(),
            config,
            authorize_url: "https://myanimelist.net/v1/oauth2/authorize".to_owned(),
            token_url: "https://myanimelist.net/v1/oauth2/token".to_owned(),
            api_url: "https://api.myanimelist.net/v2".to_owned(),
        }
    }

    pub fn with_endpoints(
        config: OAuthProviderConfig,
        authorize_url: String,
        token_url: String,
        api_url: String,
    ) -> Self {
        Self {
            http: reqwest::Client::new(),
            config,
            authorize_url,
            token_url,
            api_url,
        }
    }
}

#[async_trait]
impl AnimeProviderClient for MyAnimeListClient {
    fn provider(&self) -> AuthProvider {
        AuthProvider::MyAnimeList
    }

    fn authorization_url(&self, state: &str, pkce_challenge: Option<&str>) -> String {
        let mut url = Url::parse(&self.authorize_url).expect("static MAL authorize URL is valid");
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", &self.config.client_id)
            .append_pair("redirect_uri", &self.config.redirect_uri)
            .append_pair("code_challenge", pkce_challenge.unwrap_or(""))
            .append_pair("code_challenge_method", "plain")
            .append_pair("state", state);
        url.to_string()
    }

    async fn exchange_code(
        &self,
        code: &str,
        pkce_verifier: Option<&str>,
    ) -> Result<ProviderIdentity, ProviderError> {
        let token = self
            .http
            .post(&self.token_url)
            .form(&[
                ("client_id", self.config.client_id.as_str()),
                ("client_secret", self.config.client_secret.as_str()),
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", self.config.redirect_uri.as_str()),
                ("code_verifier", pkce_verifier.unwrap_or("")),
            ])
            .send()
            .await?
            .error_for_status()?
            .json::<OAuthTokenResponse>()
            .await?;
        let profile = self.profile(&token.access_token).await?;
        Ok(ProviderIdentity {
            provider: AuthProvider::MyAnimeList,
            provider_user_id: profile.id.to_string(),
            username: profile.name,
            avatar_url: profile.picture,
            access_token: token.access_token,
            refresh_token: token.refresh_token,
            expires_at: token
                .expires_in
                .map(|seconds| Utc::now() + Duration::seconds(seconds)),
        })
    }

    async fn sync_list(
        &self,
        identity: &ProviderIdentity,
    ) -> Result<ProviderListSync, ProviderError> {
        let access_token = if identity
            .expires_at
            .is_some_and(|expires_at| expires_at <= Utc::now())
        {
            if let Some(refresh_token) = &identity.refresh_token {
                self.refresh_access_token(refresh_token).await?.access_token
            } else {
                identity.access_token.clone()
            }
        } else {
            identity.access_token.clone()
        };
        let url = format!(
            "{}/users/@me/animelist?fields=list_status,num_episodes,title,main_picture&limit=1000",
            self.api_url
        );
        let response = self
            .http
            .get(url)
            .bearer_auth(&access_token)
            .send()
            .await?
            .error_for_status()?
            .json::<MalAnimeListResponse>()
            .await?;
        let now = Utc::now();
        let entries = response
            .data
            .into_iter()
            .map(|entry| entry.into_library_entry(identity, now))
            .collect();
        Ok(ProviderListSync {
            provider: AuthProvider::MyAnimeList,
            entries,
            synced_at: now,
        })
    }
}

impl MyAnimeListClient {
    async fn refresh_access_token(
        &self,
        refresh_token: &str,
    ) -> Result<OAuthTokenResponse, ProviderError> {
        Ok(self
            .http
            .post(&self.token_url)
            .form(&[
                ("client_id", self.config.client_id.as_str()),
                ("client_secret", self.config.client_secret.as_str()),
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
            ])
            .send()
            .await?
            .error_for_status()?
            .json::<OAuthTokenResponse>()
            .await?)
    }

    async fn profile(&self, access_token: &str) -> Result<MalProfile, ProviderError> {
        let url = format!("{}/users/@me", self.api_url);
        Ok(self
            .http
            .get(url)
            .bearer_auth(access_token)
            .send()
            .await?
            .error_for_status()?
            .json::<MalProfile>()
            .await?)
    }
}

pub fn new_mal_pkce_verifier() -> String {
    random_url_token(64)
}

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("provider rejected the oauth code")]
    InvalidCode,
    #[error("provider request failed: {0}")]
    Request(String),
    #[error("provider returned invalid data: {0}")]
    InvalidData(String),
    #[error("http request failed: {0}")]
    Http(#[from] reqwest::Error),
}

#[derive(Clone, Debug)]
pub struct ProviderSyncJob {
    pub provider: AuthProvider,
    pub provider_user_id: String,
}

#[derive(Serialize)]
struct AniListTokenRequest<'a> {
    grant_type: &'a str,
    client_id: &'a str,
    client_secret: &'a str,
    redirect_uri: &'a str,
    code: &'a str,
}

#[derive(Deserialize)]
struct OAuthTokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
}

#[derive(Serialize)]
struct GraphQlRequest<'a> {
    query: &'a str,
    variables: serde_json::Value,
}

#[derive(Deserialize)]
struct GraphQlResponse<T> {
    data: Option<T>,
    errors: Option<Vec<GraphQlError>>,
}

#[derive(Deserialize)]
struct GraphQlError {
    message: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AniListViewerData {
    viewer: AniListViewer,
}

#[derive(Deserialize)]
struct AniListViewer {
    id: i64,
    name: String,
    avatar: Option<AniListAvatar>,
}

#[derive(Deserialize)]
struct AniListAvatar {
    large: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AniListMediaListCollectionData {
    media_list_collection: AniListMediaListCollection,
}

#[derive(Deserialize)]
struct AniListMediaListCollection {
    lists: Vec<AniListMediaListGroup>,
}

#[derive(Deserialize)]
struct AniListMediaListGroup {
    entries: Vec<AniListMediaListEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AniListMediaListEntry {
    media_id: i64,
    status: Option<String>,
    score: Option<f32>,
    progress: Option<u32>,
    media: Option<AniListMedia>,
    updated_at: Option<i64>,
}

impl AniListMediaListEntry {
    fn into_library_entry(
        self,
        identity: &ProviderIdentity,
        now: DateTime<Utc>,
    ) -> Option<AnimeListEntry> {
        let media = self.media?;
        Some(AnimeListEntry {
            id: Uuid::new_v4(),
            user_id: Uuid::nil(),
            provider: identity.provider,
            provider_anime_id: self.media_id.to_string(),
            title: media
                .title
                .romaji
                .or(media.title.english)
                .unwrap_or_else(|| self.media_id.to_string()),
            status: parse_watch_status(self.status.as_deref()),
            score: self.score,
            progress_episodes: self.progress.unwrap_or_default(),
            total_episodes: media.episodes,
            updated_at: self
                .updated_at
                .and_then(|timestamp| DateTime::from_timestamp(timestamp, 0))
                .unwrap_or(now),
        })
    }
}

#[derive(Deserialize)]
struct AniListMedia {
    title: AniListTitle,
    episodes: Option<u32>,
}

#[derive(Deserialize)]
struct AniListTitle {
    romaji: Option<String>,
    english: Option<String>,
}

#[derive(Deserialize)]
struct MalProfile {
    id: i64,
    name: String,
    picture: Option<String>,
}

#[derive(Deserialize)]
struct MalAnimeListResponse {
    data: Vec<MalAnimeListItem>,
}

#[derive(Deserialize)]
struct MalAnimeListItem {
    node: MalAnimeNode,
    list_status: Option<MalListStatus>,
}

impl MalAnimeListItem {
    fn into_library_entry(self, identity: &ProviderIdentity, now: DateTime<Utc>) -> AnimeListEntry {
        let status = self.list_status.unwrap_or_default();
        AnimeListEntry {
            id: Uuid::new_v4(),
            user_id: Uuid::nil(),
            provider: identity.provider,
            provider_anime_id: self.node.id.to_string(),
            title: self.node.title,
            status: parse_watch_status(status.status.as_deref()),
            score: status.score.map(f32::from),
            progress_episodes: status.num_episodes_watched.unwrap_or_default(),
            total_episodes: self.node.num_episodes,
            updated_at: status
                .updated_at
                .and_then(|value| DateTime::parse_from_rfc3339(&value).ok())
                .map(|date| date.with_timezone(&Utc))
                .unwrap_or(now),
        }
    }
}

#[derive(Deserialize)]
struct MalAnimeNode {
    id: i64,
    title: String,
    num_episodes: Option<u32>,
}

#[derive(Default, Deserialize)]
struct MalListStatus {
    status: Option<String>,
    score: Option<u8>,
    num_episodes_watched: Option<u32>,
    updated_at: Option<String>,
}

fn parse_watch_status(status: Option<&str>) -> WatchStatus {
    match status {
        Some("CURRENT") | Some("current") | Some("watching") => WatchStatus::Watching,
        Some("COMPLETED") | Some("completed") => WatchStatus::Completed,
        Some("PAUSED") | Some("paused") | Some("on_hold") => WatchStatus::Paused,
        Some("DROPPED") | Some("dropped") => WatchStatus::Dropped,
        _ => WatchStatus::Planning,
    }
}

const ANILIST_VIEWER_QUERY: &str = r#"
query Viewer {
  Viewer { id name avatar { large } }
}
"#;

const ANILIST_LIST_QUERY: &str = r#"
query UserAnimeList($userId: Int) {
  MediaListCollection(userId: $userId, type: ANIME) {
    lists {
      entries {
        mediaId
        status
        score
        progress
        updatedAt
        media { title { romaji english } episodes }
      }
    }
  }
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mal_pkce_verifier_is_url_safe_and_large_enough() {
        let verifier = new_mal_pkce_verifier();
        assert!(verifier.len() >= 48);
        assert!(!verifier.contains('='));
    }

    #[test]
    fn anilist_authorization_url_contains_code_flow_params() {
        let client = AniListClient::new(OAuthProviderConfig {
            client_id: "client".to_owned(),
            client_secret: "secret".to_owned(),
            redirect_uri: "http://localhost/callback".to_owned(),
        });
        let url = client.authorization_url("state", None);
        assert!(url.contains("response_type=code"));
        assert!(url.contains("state=state"));
    }
}
