pub mod memory;
pub mod mongo;
pub mod sql;

use async_trait::async_trait;
use typenx_core::{
    addons::{AddonRegistration, MetadataCacheEntry},
    auth::{AuthProvider, LinkedProvider, OAuthState, Session, User},
    library::{AnimeListEntry, WatchProgress},
};
use uuid::Uuid;

pub use mongo::MongoStore;
pub use sql::{DatabaseKind, SqlStore};

#[async_trait]
pub trait TypenxStore: Send + Sync {
    async fn migrate(&self) -> Result<(), StorageError>;
    async fn upsert_user(&self, user: User) -> Result<User, StorageError>;
    async fn get_user(&self, user_id: Uuid) -> Result<Option<User>, StorageError>;
    async fn upsert_linked_provider(
        &self,
        linked_provider: LinkedProvider,
    ) -> Result<LinkedProvider, StorageError>;
    async fn list_linked_providers(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<LinkedProvider>, StorageError>;
    async fn create_session(&self, session: Session) -> Result<Session, StorageError>;
    async fn get_session_by_token_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<Session>, StorageError>;
    async fn revoke_session(&self, session_id: Uuid) -> Result<(), StorageError>;
    async fn create_oauth_state(&self, state: OAuthState) -> Result<OAuthState, StorageError>;
    async fn consume_oauth_state(
        &self,
        state: &str,
        provider: AuthProvider,
    ) -> Result<Option<OAuthState>, StorageError>;
    async fn find_linked_provider(
        &self,
        provider: AuthProvider,
        provider_user_id: &str,
    ) -> Result<Option<LinkedProvider>, StorageError>;
    async fn list_library(&self, user_id: Uuid) -> Result<Vec<AnimeListEntry>, StorageError>;
    async fn upsert_library_entry(
        &self,
        entry: AnimeListEntry,
    ) -> Result<AnimeListEntry, StorageError>;
    async fn upsert_watch_progress(
        &self,
        progress: WatchProgress,
    ) -> Result<WatchProgress, StorageError>;
    async fn list_watch_progress(&self, user_id: Uuid) -> Result<Vec<WatchProgress>, StorageError>;
    async fn register_addon(
        &self,
        addon: AddonRegistration,
    ) -> Result<AddonRegistration, StorageError>;
    async fn update_addon(
        &self,
        addon: AddonRegistration,
    ) -> Result<AddonRegistration, StorageError>;
    async fn delete_addon(&self, addon_id: Uuid) -> Result<(), StorageError>;
    async fn list_addons(&self) -> Result<Vec<AddonRegistration>, StorageError>;
    async fn get_metadata_cache(
        &self,
        addon_id: Uuid,
        cache_key: &str,
    ) -> Result<Option<MetadataCacheEntry>, StorageError>;
    async fn set_metadata_cache(
        &self,
        entry: MetadataCacheEntry,
    ) -> Result<MetadataCacheEntry, StorageError>;
}

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("mongodb error: {0}")]
    Mongo(#[from] mongodb::error::Error),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("unsupported database url: {0}")]
    UnsupportedDatabaseUrl(String),
    #[error("store lock poisoned")]
    LockPoisoned,
}
