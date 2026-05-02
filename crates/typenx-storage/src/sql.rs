use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{any::AnyPoolOptions, AnyPool, Row};
use typenx_core::{
    addons::{AddonRegistration, AddonSource, MetadataCacheEntry},
    auth::{AuthProvider, LinkedProvider, OAuthState, Session, User},
    library::{AnimeListEntry, WatchProgress, WatchStatus},
};
use uuid::Uuid;

use crate::{StorageError, TypenxStore};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DatabaseKind {
    Postgres,
    MySql,
    Sqlite,
    MongoFuture,
}

impl DatabaseKind {
    pub fn from_url(url: &str) -> Result<Self, StorageError> {
        if url.starts_with("postgres://") || url.starts_with("postgresql://") {
            Ok(Self::Postgres)
        } else if url.starts_with("mysql://") {
            Ok(Self::MySql)
        } else if url.starts_with("sqlite://") {
            Ok(Self::Sqlite)
        } else if url.starts_with("mongodb://") || url.starts_with("mongodb+srv://") {
            Ok(Self::MongoFuture)
        } else {
            Err(StorageError::UnsupportedDatabaseUrl(url.to_owned()))
        }
    }
}

#[derive(Clone)]
pub struct SqlStore {
    pool: AnyPool,
    kind: DatabaseKind,
}

impl SqlStore {
    pub async fn connect(database_url: &str) -> Result<Self, StorageError> {
        let kind = DatabaseKind::from_url(database_url)?;
        if kind == DatabaseKind::MongoFuture {
            return Err(StorageError::UnsupportedDatabaseUrl(
                "MongoDB is reserved for a future equal adapter".to_owned(),
            ));
        }
        sqlx::any::install_default_drivers();
        let pool = AnyPoolOptions::new()
            .max_connections(10)
            .connect(database_url)
            .await?;
        Ok(Self { pool, kind })
    }

    pub const fn kind(&self) -> DatabaseKind {
        self.kind
    }
}

#[async_trait]
impl TypenxStore for SqlStore {
    async fn migrate(&self) -> Result<(), StorageError> {
        for statement in MIGRATIONS {
            sqlx::query(statement).execute(&self.pool).await?;
        }
        for statement in OPTIONAL_MIGRATIONS {
            let _ = sqlx::query(statement).execute(&self.pool).await;
        }
        Ok(())
    }

    async fn upsert_user(&self, user: User) -> Result<User, StorageError> {
        sqlx::query(
            "INSERT INTO users (id, display_name, avatar_url, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                display_name = excluded.display_name,
                avatar_url = excluded.avatar_url,
                updated_at = excluded.updated_at",
        )
        .bind(user.id.to_string())
        .bind(&user.display_name)
        .bind(&user.avatar_url)
        .bind(user.created_at.to_rfc3339())
        .bind(user.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(user)
    }

    async fn get_user(&self, user_id: Uuid) -> Result<Option<User>, StorageError> {
        let row = sqlx::query(
            "SELECT id, display_name, avatar_url, created_at, updated_at FROM users WHERE id = ?",
        )
        .bind(user_id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_user).transpose()
    }

    async fn upsert_linked_provider(
        &self,
        linked_provider: LinkedProvider,
    ) -> Result<LinkedProvider, StorageError> {
        sqlx::query(
            "INSERT INTO linked_providers
             (id, user_id, provider, provider_user_id, provider_username, access_token,
              refresh_token, expires_at, linked_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(provider, provider_user_id) DO UPDATE SET
                user_id = excluded.user_id,
                provider_username = excluded.provider_username,
                access_token = excluded.access_token,
                refresh_token = excluded.refresh_token,
                expires_at = excluded.expires_at",
        )
        .bind(linked_provider.id.to_string())
        .bind(linked_provider.user_id.to_string())
        .bind(linked_provider.provider.as_str())
        .bind(&linked_provider.provider_user_id)
        .bind(&linked_provider.provider_username)
        .bind(&linked_provider.access_token)
        .bind(&linked_provider.refresh_token)
        .bind(linked_provider.expires_at.map(|date| date.to_rfc3339()))
        .bind(linked_provider.linked_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(linked_provider)
    }

    async fn list_linked_providers(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<LinkedProvider>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, user_id, provider, provider_user_id, provider_username, access_token,
                    refresh_token, expires_at, linked_at
             FROM linked_providers WHERE user_id = ?",
        )
        .bind(user_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_linked_provider).collect()
    }

    async fn create_session(&self, session: Session) -> Result<Session, StorageError> {
        sqlx::query(
            "INSERT INTO sessions
             (id, user_id, token_hash, created_at, expires_at, revoked_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(session.id.to_string())
        .bind(session.user_id.to_string())
        .bind(&session.token_hash)
        .bind(session.created_at.to_rfc3339())
        .bind(session.expires_at.to_rfc3339())
        .bind(session.revoked_at.map(|date| date.to_rfc3339()))
        .execute(&self.pool)
        .await?;
        Ok(session)
    }

    async fn get_session_by_token_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<Session>, StorageError> {
        let row = sqlx::query(
            "SELECT id, user_id, token_hash, created_at, expires_at, revoked_at
             FROM sessions WHERE token_hash = ?",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_session).transpose()
    }

    async fn revoke_session(&self, session_id: Uuid) -> Result<(), StorageError> {
        sqlx::query("UPDATE sessions SET revoked_at = ? WHERE id = ?")
            .bind(Utc::now().to_rfc3339())
            .bind(session_id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn create_oauth_state(&self, state: OAuthState) -> Result<OAuthState, StorageError> {
        sqlx::query(
            "INSERT INTO oauth_states
             (state, provider, redirect_after, pkce_verifier, created_at, expires_at, consumed_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&state.state)
        .bind(state.provider.as_str())
        .bind(&state.redirect_after)
        .bind(&state.pkce_verifier)
        .bind(state.created_at.to_rfc3339())
        .bind(state.expires_at.to_rfc3339())
        .bind(state.consumed_at.map(|date| date.to_rfc3339()))
        .execute(&self.pool)
        .await?;
        Ok(state)
    }

    async fn consume_oauth_state(
        &self,
        state: &str,
        provider: AuthProvider,
    ) -> Result<Option<OAuthState>, StorageError> {
        let row = sqlx::query(
            "SELECT state, provider, redirect_after, pkce_verifier, created_at, expires_at, consumed_at
             FROM oauth_states WHERE state = ? AND provider = ?",
        )
        .bind(state)
        .bind(provider.as_str())
        .fetch_optional(&self.pool)
        .await?;
        let Some(oauth_state) = row.map(row_to_oauth_state).transpose()? else {
            return Ok(None);
        };
        if oauth_state.consumed_at.is_some() {
            return Ok(None);
        }
        sqlx::query("UPDATE oauth_states SET consumed_at = ? WHERE state = ?")
            .bind(Utc::now().to_rfc3339())
            .bind(state)
            .execute(&self.pool)
            .await?;
        Ok(Some(oauth_state))
    }

    async fn find_linked_provider(
        &self,
        provider: AuthProvider,
        provider_user_id: &str,
    ) -> Result<Option<LinkedProvider>, StorageError> {
        let row = sqlx::query(
            "SELECT id, user_id, provider, provider_user_id, provider_username, access_token,
                    refresh_token, expires_at, linked_at
             FROM linked_providers WHERE provider = ? AND provider_user_id = ?",
        )
        .bind(provider.as_str())
        .bind(provider_user_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_linked_provider).transpose()
    }

    async fn list_library(&self, user_id: Uuid) -> Result<Vec<AnimeListEntry>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, user_id, provider, provider_anime_id, title, status, score,
                    progress_episodes, total_episodes, updated_at
             FROM anime_list_entries WHERE user_id = ?",
        )
        .bind(user_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_library_entry).collect()
    }

    async fn upsert_library_entry(
        &self,
        entry: AnimeListEntry,
    ) -> Result<AnimeListEntry, StorageError> {
        sqlx::query(
            "INSERT INTO anime_list_entries
             (id, user_id, provider, provider_anime_id, title, status, score, progress_episodes,
              total_episodes, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(user_id, provider, provider_anime_id) DO UPDATE SET
                title = excluded.title,
                status = excluded.status,
                score = excluded.score,
                progress_episodes = excluded.progress_episodes,
                total_episodes = excluded.total_episodes,
                updated_at = excluded.updated_at",
        )
        .bind(entry.id.to_string())
        .bind(entry.user_id.to_string())
        .bind(entry.provider.as_str())
        .bind(&entry.provider_anime_id)
        .bind(&entry.title)
        .bind(status_to_str(entry.status))
        .bind(entry.score)
        .bind(i64::from(entry.progress_episodes))
        .bind(entry.total_episodes.map(i64::from))
        .bind(entry.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(entry)
    }

    async fn upsert_watch_progress(
        &self,
        progress: WatchProgress,
    ) -> Result<WatchProgress, StorageError> {
        sqlx::query(
            "INSERT INTO watch_progress
             (id, user_id, anime_id, episode_id, episode_number, position_seconds,
              duration_seconds, completed, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(user_id, anime_id, episode_id) DO UPDATE SET
                episode_number = excluded.episode_number,
                position_seconds = excluded.position_seconds,
                duration_seconds = excluded.duration_seconds,
                completed = excluded.completed,
                updated_at = excluded.updated_at",
        )
        .bind(progress.id.to_string())
        .bind(progress.user_id.to_string())
        .bind(&progress.anime_id)
        .bind(&progress.episode_id)
        .bind(progress.episode_number.map(i64::from))
        .bind(i64::from(progress.position_seconds))
        .bind(progress.duration_seconds.map(i64::from))
        .bind(progress.completed)
        .bind(progress.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(progress)
    }

    async fn list_watch_progress(&self, user_id: Uuid) -> Result<Vec<WatchProgress>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, user_id, anime_id, episode_id, episode_number, position_seconds,
                    duration_seconds, CASE WHEN completed THEN 1 ELSE 0 END AS completed, updated_at
             FROM watch_progress WHERE user_id = ?",
        )
        .bind(user_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_watch_progress).collect()
    }

    async fn register_addon(
        &self,
        addon: AddonRegistration,
    ) -> Result<AddonRegistration, StorageError> {
        let manifest_json = addon
            .manifest
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        sqlx::query(
            "INSERT INTO addons (id, base_url, enabled, source, deletable, manifest_json, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(base_url) DO UPDATE SET
                enabled = excluded.enabled,
                source = excluded.source,
                deletable = excluded.deletable,
                manifest_json = excluded.manifest_json,
                updated_at = excluded.updated_at",
        )
        .bind(addon.id.to_string())
        .bind(&addon.base_url)
        .bind(addon.enabled)
        .bind(addon_source_to_str(addon.source))
        .bind(addon.deletable)
        .bind(manifest_json)
        .bind(addon.created_at.to_rfc3339())
        .bind(addon.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(addon)
    }

    async fn update_addon(
        &self,
        addon: AddonRegistration,
    ) -> Result<AddonRegistration, StorageError> {
        let manifest_json = addon
            .manifest
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        sqlx::query(
            "UPDATE addons SET base_url = ?, enabled = ?, source = ?, deletable = ?, manifest_json = ?, updated_at = ? WHERE id = ?",
        )
        .bind(&addon.base_url)
        .bind(addon.enabled)
        .bind(addon_source_to_str(addon.source))
        .bind(addon.deletable)
        .bind(manifest_json)
        .bind(addon.updated_at.to_rfc3339())
        .bind(addon.id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(addon)
    }

    async fn delete_addon(&self, addon_id: Uuid) -> Result<(), StorageError> {
        sqlx::query("DELETE FROM addons WHERE id = ?")
            .bind(addon_id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn list_addons(&self) -> Result<Vec<AddonRegistration>, StorageError> {
        let rows = sqlx::query(
            "SELECT id,
                    base_url,
                    CASE WHEN enabled THEN 1 ELSE 0 END AS enabled,
                    source,
                    CASE WHEN deletable THEN 1 ELSE 0 END AS deletable,
                    manifest_json,
                    created_at,
                    updated_at
             FROM addons",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_addon).collect()
    }

    async fn get_metadata_cache(
        &self,
        addon_id: Uuid,
        cache_key: &str,
    ) -> Result<Option<MetadataCacheEntry>, StorageError> {
        let row = sqlx::query(
            "SELECT id, addon_id, cache_key, payload_json, expires_at, created_at
             FROM metadata_cache WHERE addon_id = ? AND cache_key = ?",
        )
        .bind(addon_id.to_string())
        .bind(cache_key)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_metadata_cache).transpose()
    }

    async fn set_metadata_cache(
        &self,
        entry: MetadataCacheEntry,
    ) -> Result<MetadataCacheEntry, StorageError> {
        sqlx::query(
            "INSERT INTO metadata_cache (id, addon_id, cache_key, payload_json, expires_at, created_at)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT(addon_id, cache_key) DO UPDATE SET
                payload_json = excluded.payload_json,
                expires_at = excluded.expires_at",
        )
        .bind(entry.id.to_string())
        .bind(entry.addon_id.to_string())
        .bind(&entry.cache_key)
        .bind(&entry.payload_json)
        .bind(entry.expires_at.to_rfc3339())
        .bind(entry.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(entry)
    }
}

const MIGRATIONS: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS users (
        id TEXT PRIMARY KEY,
        display_name TEXT NOT NULL,
        avatar_url TEXT NULL,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
    )",
    "CREATE TABLE IF NOT EXISTS linked_providers (
        id TEXT PRIMARY KEY,
        user_id TEXT NOT NULL,
        provider TEXT NOT NULL,
        provider_user_id TEXT NOT NULL,
        provider_username TEXT NOT NULL,
        access_token TEXT NOT NULL,
        refresh_token TEXT NULL,
        expires_at TEXT NULL,
        linked_at TEXT NOT NULL,
        UNIQUE(provider, provider_user_id)
    )",
    "CREATE UNIQUE INDEX IF NOT EXISTS linked_providers_identity_idx
        ON linked_providers (provider, provider_user_id)",
    "CREATE TABLE IF NOT EXISTS sessions (
        id TEXT PRIMARY KEY,
        user_id TEXT NOT NULL,
        token_hash TEXT NOT NULL,
        created_at TEXT NOT NULL,
        expires_at TEXT NOT NULL,
        revoked_at TEXT NULL
    )",
    "CREATE INDEX IF NOT EXISTS sessions_token_hash_idx ON sessions (token_hash)",
    "CREATE TABLE IF NOT EXISTS oauth_states (
        state TEXT PRIMARY KEY,
        provider TEXT NOT NULL,
        redirect_after TEXT NULL,
        pkce_verifier TEXT NULL,
        created_at TEXT NOT NULL,
        expires_at TEXT NOT NULL,
        consumed_at TEXT NULL
    )",
    "CREATE TABLE IF NOT EXISTS anime_list_entries (
        id TEXT PRIMARY KEY,
        user_id TEXT NOT NULL,
        provider TEXT NOT NULL,
        provider_anime_id TEXT NOT NULL,
        title TEXT NOT NULL,
        status TEXT NOT NULL,
        score REAL NULL,
        progress_episodes INTEGER NOT NULL,
        total_episodes INTEGER NULL,
        updated_at TEXT NOT NULL,
        UNIQUE(user_id, provider, provider_anime_id)
    )",
    "CREATE UNIQUE INDEX IF NOT EXISTS anime_list_identity_idx
        ON anime_list_entries (user_id, provider, provider_anime_id)",
    "CREATE TABLE IF NOT EXISTS watch_progress (
        id TEXT PRIMARY KEY,
        user_id TEXT NOT NULL,
        anime_id TEXT NOT NULL,
        episode_id TEXT NULL,
        episode_number INTEGER NULL,
        position_seconds INTEGER NOT NULL,
        duration_seconds INTEGER NULL,
        completed BOOLEAN NOT NULL,
        updated_at TEXT NOT NULL,
        UNIQUE(user_id, anime_id, episode_id)
    )",
    "CREATE UNIQUE INDEX IF NOT EXISTS watch_progress_identity_idx
        ON watch_progress (user_id, anime_id, episode_id)",
    "CREATE TABLE IF NOT EXISTS addons (
        id TEXT PRIMARY KEY,
        base_url TEXT NOT NULL,
        enabled BOOLEAN NOT NULL,
        source TEXT NOT NULL DEFAULT 'user',
        deletable BOOLEAN NOT NULL DEFAULT TRUE,
        manifest_json TEXT NULL,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        UNIQUE(base_url)
    )",
    "CREATE UNIQUE INDEX IF NOT EXISTS addons_base_url_idx ON addons (base_url)",
    "CREATE TABLE IF NOT EXISTS metadata_cache (
        id TEXT PRIMARY KEY,
        addon_id TEXT NOT NULL,
        cache_key TEXT NOT NULL,
        payload_json TEXT NOT NULL,
        expires_at TEXT NOT NULL,
        created_at TEXT NOT NULL,
        UNIQUE(addon_id, cache_key)
    )",
    "CREATE UNIQUE INDEX IF NOT EXISTS metadata_cache_identity_idx
        ON metadata_cache (addon_id, cache_key)",
];

const OPTIONAL_MIGRATIONS: &[&str] = &[
    "ALTER TABLE addons ADD COLUMN source TEXT NOT NULL DEFAULT 'user'",
    "ALTER TABLE addons ADD COLUMN deletable BOOLEAN NOT NULL DEFAULT TRUE",
];

fn row_to_user(row: sqlx::any::AnyRow) -> Result<User, StorageError> {
    Ok(User {
        id: parse_uuid(row.try_get::<String, _>("id")?)?,
        display_name: row.try_get("display_name")?,
        avatar_url: row.try_get("avatar_url")?,
        created_at: parse_datetime(row.try_get::<String, _>("created_at")?)?,
        updated_at: parse_datetime(row.try_get::<String, _>("updated_at")?)?,
    })
}

fn row_to_session(row: sqlx::any::AnyRow) -> Result<Session, StorageError> {
    Ok(Session {
        id: parse_uuid(row.try_get::<String, _>("id")?)?,
        user_id: parse_uuid(row.try_get::<String, _>("user_id")?)?,
        token_hash: row.try_get("token_hash")?,
        created_at: parse_datetime(row.try_get::<String, _>("created_at")?)?,
        expires_at: parse_datetime(row.try_get::<String, _>("expires_at")?)?,
        revoked_at: parse_optional_datetime(row.try_get("revoked_at")?)?,
    })
}

fn row_to_oauth_state(row: sqlx::any::AnyRow) -> Result<OAuthState, StorageError> {
    Ok(OAuthState {
        state: row.try_get("state")?,
        provider: parse_provider(&row.try_get::<String, _>("provider")?),
        redirect_after: row.try_get("redirect_after")?,
        pkce_verifier: row.try_get("pkce_verifier")?,
        created_at: parse_datetime(row.try_get::<String, _>("created_at")?)?,
        expires_at: parse_datetime(row.try_get::<String, _>("expires_at")?)?,
        consumed_at: parse_optional_datetime(row.try_get("consumed_at")?)?,
    })
}

fn row_to_linked_provider(row: sqlx::any::AnyRow) -> Result<LinkedProvider, StorageError> {
    Ok(LinkedProvider {
        id: parse_uuid(row.try_get::<String, _>("id")?)?,
        user_id: parse_uuid(row.try_get::<String, _>("user_id")?)?,
        provider: parse_provider(&row.try_get::<String, _>("provider")?),
        provider_user_id: row.try_get("provider_user_id")?,
        provider_username: row.try_get("provider_username")?,
        access_token: row.try_get("access_token")?,
        refresh_token: row.try_get("refresh_token")?,
        expires_at: parse_optional_datetime(row.try_get("expires_at")?)?,
        linked_at: parse_datetime(row.try_get::<String, _>("linked_at")?)?,
    })
}

fn row_to_library_entry(row: sqlx::any::AnyRow) -> Result<AnimeListEntry, StorageError> {
    let score = row
        .try_get::<Option<f64>, _>("score")?
        .map(|value| value as f32);

    Ok(AnimeListEntry {
        id: parse_uuid(row.try_get::<String, _>("id")?)?,
        user_id: parse_uuid(row.try_get::<String, _>("user_id")?)?,
        provider: parse_provider(&row.try_get::<String, _>("provider")?),
        provider_anime_id: row.try_get("provider_anime_id")?,
        title: row.try_get("title")?,
        status: parse_status(&row.try_get::<String, _>("status")?),
        score,
        progress_episodes: row.try_get::<i64, _>("progress_episodes")? as u32,
        total_episodes: row
            .try_get::<Option<i64>, _>("total_episodes")?
            .map(|value| value as u32),
        updated_at: parse_datetime(row.try_get::<String, _>("updated_at")?)?,
    })
}

fn row_to_watch_progress(row: sqlx::any::AnyRow) -> Result<WatchProgress, StorageError> {
    Ok(WatchProgress {
        id: parse_uuid(row.try_get::<String, _>("id")?)?,
        user_id: parse_uuid(row.try_get::<String, _>("user_id")?)?,
        anime_id: row.try_get("anime_id")?,
        episode_id: row.try_get("episode_id")?,
        episode_number: row
            .try_get::<Option<i64>, _>("episode_number")?
            .map(|value| value as u32),
        position_seconds: row.try_get::<i64, _>("position_seconds")? as u32,
        duration_seconds: row
            .try_get::<Option<i64>, _>("duration_seconds")?
            .map(|value| value as u32),
        completed: int_to_bool(row.try_get::<i64, _>("completed")?),
        updated_at: parse_datetime(row.try_get::<String, _>("updated_at")?)?,
    })
}

fn row_to_addon(row: sqlx::any::AnyRow) -> Result<AddonRegistration, StorageError> {
    let manifest_json: Option<String> = row.try_get("manifest_json")?;
    Ok(AddonRegistration {
        id: parse_uuid(row.try_get::<String, _>("id")?)?,
        base_url: row.try_get("base_url")?,
        enabled: int_to_bool(row.try_get::<i64, _>("enabled")?),
        source: parse_addon_source(&row.try_get::<String, _>("source")?),
        deletable: int_to_bool(row.try_get::<i64, _>("deletable")?),
        manifest: manifest_json
            .map(|json| serde_json::from_str(&json))
            .transpose()?,
        created_at: parse_datetime(row.try_get::<String, _>("created_at")?)?,
        updated_at: parse_datetime(row.try_get::<String, _>("updated_at")?)?,
    })
}

fn row_to_metadata_cache(row: sqlx::any::AnyRow) -> Result<MetadataCacheEntry, StorageError> {
    Ok(MetadataCacheEntry {
        id: parse_uuid(row.try_get::<String, _>("id")?)?,
        addon_id: parse_uuid(row.try_get::<String, _>("addon_id")?)?,
        cache_key: row.try_get("cache_key")?,
        payload_json: row.try_get("payload_json")?,
        expires_at: parse_datetime(row.try_get::<String, _>("expires_at")?)?,
        created_at: parse_datetime(row.try_get::<String, _>("created_at")?)?,
    })
}

fn int_to_bool(value: i64) -> bool {
    value != 0
}

fn parse_uuid(value: String) -> Result<Uuid, StorageError> {
    Uuid::parse_str(&value).map_err(|error| StorageError::UnsupportedDatabaseUrl(error.to_string()))
}

fn parse_datetime(value: String) -> Result<DateTime<Utc>, StorageError> {
    DateTime::parse_from_rfc3339(&value)
        .map(|date| date.with_timezone(&Utc))
        .map_err(|error| StorageError::UnsupportedDatabaseUrl(error.to_string()))
}

fn parse_optional_datetime(value: Option<String>) -> Result<Option<DateTime<Utc>>, StorageError> {
    value.map(parse_datetime).transpose()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_supported_database_urls() {
        assert_eq!(
            DatabaseKind::from_url("sqlite://test.db").unwrap(),
            DatabaseKind::Sqlite
        );
        assert_eq!(
            DatabaseKind::from_url("postgres://localhost/db").unwrap(),
            DatabaseKind::Postgres
        );
        assert_eq!(
            DatabaseKind::from_url("mysql://localhost/db").unwrap(),
            DatabaseKind::MySql
        );
        assert_eq!(
            DatabaseKind::from_url("mongodb://localhost/db").unwrap(),
            DatabaseKind::MongoFuture
        );
    }
}
