use async_trait::async_trait;
use chrono::{DateTime, Utc};
use mongodb::{
    bson::{doc, Document},
    options::{ClientOptions, IndexOptions},
    Client, Collection, Database, IndexModel,
};
use serde::{Deserialize, Serialize};
use typenx_core::{
    addons::{AddonRegistration, AddonSource, MetadataCacheEntry},
    auth::{AuthProvider, LinkedProvider, OAuthState, Session, User},
    library::{AnimeListEntry, WatchProgress, WatchStatus},
};
use uuid::Uuid;

use crate::{StorageError, TypenxStore};

#[derive(Clone)]
pub struct MongoStore {
    db: Database,
}

impl MongoStore {
    pub async fn connect(database_url: &str) -> Result<Self, StorageError> {
        let options = ClientOptions::parse(database_url).await?;
        let database_name = options
            .default_database
            .clone()
            .unwrap_or_else(|| "typenx".to_owned());
        let client = Client::with_options(options)?;
        Ok(Self {
            db: client.database(&database_name),
        })
    }

    fn users(&self) -> Collection<UserDoc> {
        self.db.collection("users")
    }

    fn linked_providers(&self) -> Collection<LinkedProviderDoc> {
        self.db.collection("linked_providers")
    }

    fn sessions(&self) -> Collection<SessionDoc> {
        self.db.collection("sessions")
    }

    fn oauth_states(&self) -> Collection<OAuthStateDoc> {
        self.db.collection("oauth_states")
    }

    fn library(&self) -> Collection<AnimeListEntryDoc> {
        self.db.collection("anime_list_entries")
    }

    fn progress(&self) -> Collection<WatchProgressDoc> {
        self.db.collection("watch_progress")
    }

    fn addons(&self) -> Collection<AddonRegistrationDoc> {
        self.db.collection("addons")
    }

    fn metadata_cache(&self) -> Collection<MetadataCacheEntryDoc> {
        self.db.collection("metadata_cache")
    }
}

#[async_trait]
impl TypenxStore for MongoStore {
    async fn migrate(&self) -> Result<(), StorageError> {
        create_unique_index(
            &self.linked_providers(),
            doc! { "provider": 1, "provider_user_id": 1 },
        )
        .await?;
        create_unique_index(&self.sessions(), doc! { "token_hash": 1 }).await?;
        create_unique_index(
            &self.library(),
            doc! { "user_id": 1, "provider": 1, "provider_anime_id": 1 },
        )
        .await?;
        create_unique_index(
            &self.progress(),
            doc! { "user_id": 1, "anime_id": 1, "episode_id": 1 },
        )
        .await?;
        create_unique_index(&self.addons(), doc! { "base_url": 1 }).await?;
        create_unique_index(
            &self.metadata_cache(),
            doc! { "addon_id": 1, "cache_key": 1 },
        )
        .await?;
        Ok(())
    }

    async fn upsert_user(&self, user: User) -> Result<User, StorageError> {
        upsert(
            &self.users(),
            doc! { "_id": user.id.to_string() },
            UserDoc::from(user.clone()),
        )
        .await?;
        Ok(user)
    }

    async fn get_user(&self, user_id: Uuid) -> Result<Option<User>, StorageError> {
        self.users()
            .find_one(doc! { "_id": user_id.to_string() })
            .await?
            .map(TryInto::try_into)
            .transpose()
    }

    async fn upsert_linked_provider(
        &self,
        linked_provider: LinkedProvider,
    ) -> Result<LinkedProvider, StorageError> {
        upsert(
            &self.linked_providers(),
            doc! {
                "provider": linked_provider.provider.as_str(),
                "provider_user_id": &linked_provider.provider_user_id,
            },
            LinkedProviderDoc::from(linked_provider.clone()),
        )
        .await?;
        Ok(linked_provider)
    }

    async fn list_linked_providers(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<LinkedProvider>, StorageError> {
        collect(
            self.linked_providers()
                .find(doc! { "user_id": user_id.to_string() })
                .await?,
        )
        .await
    }

    async fn create_session(&self, session: Session) -> Result<Session, StorageError> {
        self.sessions()
            .insert_one(SessionDoc::from(session.clone()))
            .await?;
        Ok(session)
    }

    async fn get_session_by_token_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<Session>, StorageError> {
        self.sessions()
            .find_one(doc! { "token_hash": token_hash })
            .await?
            .map(TryInto::try_into)
            .transpose()
    }

    async fn revoke_session(&self, session_id: Uuid) -> Result<(), StorageError> {
        self.sessions()
            .update_one(
                doc! { "_id": session_id.to_string() },
                doc! { "$set": { "revoked_at": Utc::now().to_rfc3339() } },
            )
            .await?;
        Ok(())
    }

    async fn create_oauth_state(&self, state: OAuthState) -> Result<OAuthState, StorageError> {
        self.oauth_states()
            .insert_one(OAuthStateDoc::from(state.clone()))
            .await?;
        Ok(state)
    }

    async fn consume_oauth_state(
        &self,
        state: &str,
        provider: AuthProvider,
    ) -> Result<Option<OAuthState>, StorageError> {
        let row = self
            .oauth_states()
            .find_one(doc! { "_id": state, "provider": provider.as_str() })
            .await?;
        let Some(doc) = row else {
            return Ok(None);
        };
        let oauth_state: OAuthState = doc.try_into()?;
        if oauth_state.consumed_at.is_some() || oauth_state.expires_at <= Utc::now() {
            return Ok(None);
        }
        self.oauth_states()
            .update_one(
                doc! { "_id": state },
                doc! { "$set": { "consumed_at": Utc::now().to_rfc3339() } },
            )
            .await?;
        Ok(Some(oauth_state))
    }

    async fn find_linked_provider(
        &self,
        provider: AuthProvider,
        provider_user_id: &str,
    ) -> Result<Option<LinkedProvider>, StorageError> {
        self.linked_providers()
            .find_one(doc! { "provider": provider.as_str(), "provider_user_id": provider_user_id })
            .await?
            .map(TryInto::try_into)
            .transpose()
    }

    async fn list_library(&self, user_id: Uuid) -> Result<Vec<AnimeListEntry>, StorageError> {
        collect(
            self.library()
                .find(doc! { "user_id": user_id.to_string() })
                .await?,
        )
        .await
    }

    async fn upsert_library_entry(
        &self,
        entry: AnimeListEntry,
    ) -> Result<AnimeListEntry, StorageError> {
        upsert(
            &self.library(),
            doc! {
                "user_id": entry.user_id.to_string(),
                "provider": entry.provider.as_str(),
                "provider_anime_id": &entry.provider_anime_id,
            },
            AnimeListEntryDoc::from(entry.clone()),
        )
        .await?;
        Ok(entry)
    }

    async fn upsert_watch_progress(
        &self,
        progress: WatchProgress,
    ) -> Result<WatchProgress, StorageError> {
        upsert(
            &self.progress(),
            doc! {
                "user_id": progress.user_id.to_string(),
                "anime_id": &progress.anime_id,
                "episode_id": &progress.episode_id,
            },
            WatchProgressDoc::from(progress.clone()),
        )
        .await?;
        Ok(progress)
    }

    async fn list_watch_progress(&self, user_id: Uuid) -> Result<Vec<WatchProgress>, StorageError> {
        collect(
            self.progress()
                .find(doc! { "user_id": user_id.to_string() })
                .await?,
        )
        .await
    }

    async fn register_addon(
        &self,
        addon: AddonRegistration,
    ) -> Result<AddonRegistration, StorageError> {
        upsert(
            &self.addons(),
            doc! { "base_url": &addon.base_url },
            AddonRegistrationDoc::from(addon.clone()),
        )
        .await?;
        Ok(addon)
    }

    async fn update_addon(
        &self,
        addon: AddonRegistration,
    ) -> Result<AddonRegistration, StorageError> {
        upsert(
            &self.addons(),
            doc! { "_id": addon.id.to_string() },
            AddonRegistrationDoc::from(addon.clone()),
        )
        .await?;
        Ok(addon)
    }

    async fn delete_addon(&self, addon_id: Uuid) -> Result<(), StorageError> {
        self.addons()
            .delete_one(doc! { "_id": addon_id.to_string() })
            .await?;
        Ok(())
    }

    async fn list_addons(&self) -> Result<Vec<AddonRegistration>, StorageError> {
        collect(self.addons().find(doc! {}).await?).await
    }

    async fn get_metadata_cache(
        &self,
        addon_id: Uuid,
        cache_key: &str,
    ) -> Result<Option<MetadataCacheEntry>, StorageError> {
        self.metadata_cache()
            .find_one(doc! { "addon_id": addon_id.to_string(), "cache_key": cache_key })
            .await?
            .map(TryInto::try_into)
            .transpose()
    }

    async fn set_metadata_cache(
        &self,
        entry: MetadataCacheEntry,
    ) -> Result<MetadataCacheEntry, StorageError> {
        upsert(
            &self.metadata_cache(),
            doc! { "addon_id": entry.addon_id.to_string(), "cache_key": &entry.cache_key },
            MetadataCacheEntryDoc::from(entry.clone()),
        )
        .await?;
        Ok(entry)
    }
}

async fn create_unique_index<T: Send + Sync>(
    collection: &Collection<T>,
    keys: Document,
) -> Result<(), StorageError> {
    let options = IndexOptions::builder().unique(true).build();
    collection
        .create_index(IndexModel::builder().keys(keys).options(options).build())
        .await?;
    Ok(())
}

async fn upsert<T>(
    collection: &Collection<T>,
    filter: Document,
    replacement: T,
) -> Result<(), StorageError>
where
    T: Serialize + Send + Sync,
{
    collection
        .replace_one(filter, replacement)
        .upsert(true)
        .await?;
    Ok(())
}

async fn collect<T, U>(cursor: mongodb::Cursor<T>) -> Result<Vec<U>, StorageError>
where
    T: for<'de> Deserialize<'de> + TryInto<U, Error = StorageError> + Unpin + Send + Sync,
    U: Send + Sync,
{
    let mut cursor = cursor;
    let mut items = Vec::new();
    while cursor.advance().await? {
        items.push(cursor.deserialize_current()?.try_into()?);
    }
    Ok(items)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct UserDoc {
    #[serde(rename = "_id")]
    id: String,
    display_name: String,
    avatar_url: Option<String>,
    created_at: String,
    updated_at: String,
}

impl From<User> for UserDoc {
    fn from(value: User) -> Self {
        Self {
            id: value.id.to_string(),
            display_name: value.display_name,
            avatar_url: value.avatar_url,
            created_at: value.created_at.to_rfc3339(),
            updated_at: value.updated_at.to_rfc3339(),
        }
    }
}

impl TryFrom<UserDoc> for User {
    type Error = StorageError;

    fn try_from(value: UserDoc) -> Result<Self, Self::Error> {
        Ok(Self {
            id: parse_uuid(&value.id)?,
            display_name: value.display_name,
            avatar_url: value.avatar_url,
            created_at: parse_datetime(&value.created_at)?,
            updated_at: parse_datetime(&value.updated_at)?,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct LinkedProviderDoc {
    #[serde(rename = "_id")]
    id: String,
    user_id: String,
    provider: String,
    provider_user_id: String,
    provider_username: String,
    access_token: String,
    refresh_token: Option<String>,
    expires_at: Option<String>,
    linked_at: String,
}

impl From<LinkedProvider> for LinkedProviderDoc {
    fn from(value: LinkedProvider) -> Self {
        Self {
            id: value.id.to_string(),
            user_id: value.user_id.to_string(),
            provider: value.provider.as_str().to_owned(),
            provider_user_id: value.provider_user_id,
            provider_username: value.provider_username,
            access_token: value.access_token,
            refresh_token: value.refresh_token,
            expires_at: value.expires_at.map(|date| date.to_rfc3339()),
            linked_at: value.linked_at.to_rfc3339(),
        }
    }
}

impl TryFrom<LinkedProviderDoc> for LinkedProvider {
    type Error = StorageError;

    fn try_from(value: LinkedProviderDoc) -> Result<Self, Self::Error> {
        Ok(Self {
            id: parse_uuid(&value.id)?,
            user_id: parse_uuid(&value.user_id)?,
            provider: parse_provider(&value.provider),
            provider_user_id: value.provider_user_id,
            provider_username: value.provider_username,
            access_token: value.access_token,
            refresh_token: value.refresh_token,
            expires_at: value
                .expires_at
                .as_deref()
                .map(parse_datetime)
                .transpose()?,
            linked_at: parse_datetime(&value.linked_at)?,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct SessionDoc {
    #[serde(rename = "_id")]
    id: String,
    user_id: String,
    token_hash: String,
    created_at: String,
    expires_at: String,
    revoked_at: Option<String>,
}

impl From<Session> for SessionDoc {
    fn from(value: Session) -> Self {
        Self {
            id: value.id.to_string(),
            user_id: value.user_id.to_string(),
            token_hash: value.token_hash,
            created_at: value.created_at.to_rfc3339(),
            expires_at: value.expires_at.to_rfc3339(),
            revoked_at: value.revoked_at.map(|date| date.to_rfc3339()),
        }
    }
}

impl TryFrom<SessionDoc> for Session {
    type Error = StorageError;

    fn try_from(value: SessionDoc) -> Result<Self, Self::Error> {
        Ok(Self {
            id: parse_uuid(&value.id)?,
            user_id: parse_uuid(&value.user_id)?,
            token_hash: value.token_hash,
            created_at: parse_datetime(&value.created_at)?,
            expires_at: parse_datetime(&value.expires_at)?,
            revoked_at: value
                .revoked_at
                .as_deref()
                .map(parse_datetime)
                .transpose()?,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct OAuthStateDoc {
    #[serde(rename = "_id")]
    state: String,
    provider: String,
    redirect_after: Option<String>,
    pkce_verifier: Option<String>,
    created_at: String,
    expires_at: String,
    consumed_at: Option<String>,
}

impl From<OAuthState> for OAuthStateDoc {
    fn from(value: OAuthState) -> Self {
        Self {
            state: value.state,
            provider: value.provider.as_str().to_owned(),
            redirect_after: value.redirect_after,
            pkce_verifier: value.pkce_verifier,
            created_at: value.created_at.to_rfc3339(),
            expires_at: value.expires_at.to_rfc3339(),
            consumed_at: value.consumed_at.map(|date| date.to_rfc3339()),
        }
    }
}

impl TryFrom<OAuthStateDoc> for OAuthState {
    type Error = StorageError;

    fn try_from(value: OAuthStateDoc) -> Result<Self, Self::Error> {
        Ok(Self {
            state: value.state,
            provider: parse_provider(&value.provider),
            redirect_after: value.redirect_after,
            pkce_verifier: value.pkce_verifier,
            created_at: parse_datetime(&value.created_at)?,
            expires_at: parse_datetime(&value.expires_at)?,
            consumed_at: value
                .consumed_at
                .as_deref()
                .map(parse_datetime)
                .transpose()?,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct AnimeListEntryDoc {
    #[serde(rename = "_id")]
    id: String,
    user_id: String,
    provider: String,
    provider_anime_id: String,
    title: String,
    status: String,
    score: Option<f32>,
    progress_episodes: u32,
    total_episodes: Option<u32>,
    updated_at: String,
}

impl From<AnimeListEntry> for AnimeListEntryDoc {
    fn from(value: AnimeListEntry) -> Self {
        Self {
            id: value.id.to_string(),
            user_id: value.user_id.to_string(),
            provider: value.provider.as_str().to_owned(),
            provider_anime_id: value.provider_anime_id,
            title: value.title,
            status: status_to_str(value.status).to_owned(),
            score: value.score,
            progress_episodes: value.progress_episodes,
            total_episodes: value.total_episodes,
            updated_at: value.updated_at.to_rfc3339(),
        }
    }
}

impl TryFrom<AnimeListEntryDoc> for AnimeListEntry {
    type Error = StorageError;

    fn try_from(value: AnimeListEntryDoc) -> Result<Self, Self::Error> {
        Ok(Self {
            id: parse_uuid(&value.id)?,
            user_id: parse_uuid(&value.user_id)?,
            provider: parse_provider(&value.provider),
            provider_anime_id: value.provider_anime_id,
            title: value.title,
            status: parse_status(&value.status),
            score: value.score,
            progress_episodes: value.progress_episodes,
            total_episodes: value.total_episodes,
            updated_at: parse_datetime(&value.updated_at)?,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct WatchProgressDoc {
    #[serde(rename = "_id")]
    id: String,
    user_id: String,
    anime_id: String,
    episode_id: Option<String>,
    episode_number: Option<u32>,
    position_seconds: u32,
    duration_seconds: Option<u32>,
    completed: bool,
    updated_at: String,
}

impl From<WatchProgress> for WatchProgressDoc {
    fn from(value: WatchProgress) -> Self {
        Self {
            id: value.id.to_string(),
            user_id: value.user_id.to_string(),
            anime_id: value.anime_id,
            episode_id: value.episode_id,
            episode_number: value.episode_number,
            position_seconds: value.position_seconds,
            duration_seconds: value.duration_seconds,
            completed: value.completed,
            updated_at: value.updated_at.to_rfc3339(),
        }
    }
}

impl TryFrom<WatchProgressDoc> for WatchProgress {
    type Error = StorageError;

    fn try_from(value: WatchProgressDoc) -> Result<Self, Self::Error> {
        Ok(Self {
            id: parse_uuid(&value.id)?,
            user_id: parse_uuid(&value.user_id)?,
            anime_id: value.anime_id,
            episode_id: value.episode_id,
            episode_number: value.episode_number,
            position_seconds: value.position_seconds,
            duration_seconds: value.duration_seconds,
            completed: value.completed,
            updated_at: parse_datetime(&value.updated_at)?,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct AddonRegistrationDoc {
    #[serde(rename = "_id")]
    id: String,
    base_url: String,
    enabled: bool,
    source: String,
    deletable: bool,
    manifest_json: Option<String>,
    created_at: String,
    updated_at: String,
}

impl From<AddonRegistration> for AddonRegistrationDoc {
    fn from(value: AddonRegistration) -> Self {
        Self {
            id: value.id.to_string(),
            base_url: value.base_url,
            enabled: value.enabled,
            source: addon_source_to_str(value.source).to_owned(),
            deletable: value.deletable,
            manifest_json: value
                .manifest
                .map(|manifest| serde_json::to_string(&manifest))
                .transpose()
                .ok()
                .flatten(),
            created_at: value.created_at.to_rfc3339(),
            updated_at: value.updated_at.to_rfc3339(),
        }
    }
}

impl TryFrom<AddonRegistrationDoc> for AddonRegistration {
    type Error = StorageError;

    fn try_from(value: AddonRegistrationDoc) -> Result<Self, Self::Error> {
        Ok(Self {
            id: parse_uuid(&value.id)?,
            base_url: value.base_url,
            enabled: value.enabled,
            source: parse_addon_source(&value.source),
            deletable: value.deletable,
            manifest: value
                .manifest_json
                .map(|json| serde_json::from_str(&json))
                .transpose()?,
            created_at: parse_datetime(&value.created_at)?,
            updated_at: parse_datetime(&value.updated_at)?,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct MetadataCacheEntryDoc {
    #[serde(rename = "_id")]
    id: String,
    addon_id: String,
    cache_key: String,
    payload_json: String,
    expires_at: String,
    created_at: String,
}

impl From<MetadataCacheEntry> for MetadataCacheEntryDoc {
    fn from(value: MetadataCacheEntry) -> Self {
        Self {
            id: value.id.to_string(),
            addon_id: value.addon_id.to_string(),
            cache_key: value.cache_key,
            payload_json: value.payload_json,
            expires_at: value.expires_at.to_rfc3339(),
            created_at: value.created_at.to_rfc3339(),
        }
    }
}

impl TryFrom<MetadataCacheEntryDoc> for MetadataCacheEntry {
    type Error = StorageError;

    fn try_from(value: MetadataCacheEntryDoc) -> Result<Self, Self::Error> {
        Ok(Self {
            id: parse_uuid(&value.id)?,
            addon_id: parse_uuid(&value.addon_id)?,
            cache_key: value.cache_key,
            payload_json: value.payload_json,
            expires_at: parse_datetime(&value.expires_at)?,
            created_at: parse_datetime(&value.created_at)?,
        })
    }
}

fn parse_uuid(value: &str) -> Result<Uuid, StorageError> {
    Uuid::parse_str(value).map_err(|error| StorageError::UnsupportedDatabaseUrl(error.to_string()))
}

fn parse_datetime(value: &str) -> Result<DateTime<Utc>, StorageError> {
    DateTime::parse_from_rfc3339(value)
        .map(|date| date.with_timezone(&Utc))
        .map_err(|error| StorageError::UnsupportedDatabaseUrl(error.to_string()))
}

fn parse_provider(value: &str) -> AuthProvider {
    match value {
        "my_anime_list" => AuthProvider::MyAnimeList,
        _ => AuthProvider::AniList,
    }
}

fn addon_source_to_str(source: AddonSource) -> &'static str {
    match source {
        AddonSource::BuiltIn => "built_in",
        AddonSource::User => "user",
    }
}

fn parse_addon_source(value: &str) -> AddonSource {
    match value {
        "built_in" => AddonSource::BuiltIn,
        _ => AddonSource::User,
    }
}

fn status_to_str(status: WatchStatus) -> &'static str {
    match status {
        WatchStatus::Planning => "planning",
        WatchStatus::Watching => "watching",
        WatchStatus::Completed => "completed",
        WatchStatus::Paused => "paused",
        WatchStatus::Dropped => "dropped",
    }
}

fn parse_status(value: &str) -> WatchStatus {
    match value {
        "watching" => WatchStatus::Watching,
        "completed" => WatchStatus::Completed,
        "paused" => WatchStatus::Paused,
        "dropped" => WatchStatus::Dropped,
        _ => WatchStatus::Planning,
    }
}
