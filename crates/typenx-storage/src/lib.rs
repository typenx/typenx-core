pub mod memory;
pub mod sql;

use async_trait::async_trait;
use typenx_core::{
    addons::AddonRegistration,
    auth::{LinkedProvider, Session, User},
    library::{AnimeListEntry, WatchProgress},
};
use uuid::Uuid;

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
    async fn list_addons(&self) -> Result<Vec<AddonRegistration>, StorageError>;
}

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("unsupported database url: {0}")]
    UnsupportedDatabaseUrl(String),
    #[error("store lock poisoned")]
    LockPoisoned,
}
