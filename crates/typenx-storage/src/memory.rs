use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use async_trait::async_trait;
use typenx_core::{
    addons::{AddonRegistration, MetadataCacheEntry},
    auth::{AuthProvider, LinkedProvider, OAuthState, Session, User},
    library::{AnimeListEntry, WatchProgress},
};
use uuid::Uuid;

use crate::{StorageError, TypenxStore};

#[derive(Clone, Default)]
pub struct MemoryStore {
    inner: Arc<RwLock<MemoryState>>,
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use uuid::Uuid;

    use super::*;
    use typenx_core::auth::AuthProvider;

    #[tokio::test]
    async fn stores_users_and_linked_providers() {
        let store = MemoryStore::default();
        let now = Utc::now();
        let user = User {
            id: Uuid::new_v4(),
            display_name: "Aki".to_owned(),
            avatar_url: None,
            created_at: now,
            updated_at: now,
        };
        let provider = LinkedProvider {
            id: Uuid::new_v4(),
            user_id: user.id,
            provider: AuthProvider::AniList,
            provider_user_id: "42".to_owned(),
            provider_username: "aki".to_owned(),
            access_token: "token".to_owned(),
            refresh_token: None,
            expires_at: None,
            linked_at: now,
        };

        store.upsert_user(user.clone()).await.unwrap();
        store
            .upsert_linked_provider(provider.clone())
            .await
            .unwrap();

        assert_eq!(store.get_user(user.id).await.unwrap(), Some(user));
        assert_eq!(
            store.list_linked_providers(provider.user_id).await.unwrap(),
            vec![provider]
        );
    }
}

#[derive(Default)]
struct MemoryState {
    users: HashMap<Uuid, User>,
    linked_providers: HashMap<Uuid, LinkedProvider>,
    sessions: HashMap<Uuid, Session>,
    oauth_states: HashMap<String, OAuthState>,
    library: HashMap<Uuid, AnimeListEntry>,
    progress: HashMap<Uuid, WatchProgress>,
    addons: HashMap<Uuid, AddonRegistration>,
    metadata_cache: HashMap<(Uuid, String), MetadataCacheEntry>,
}

#[async_trait]
impl TypenxStore for MemoryStore {
    async fn migrate(&self) -> Result<(), StorageError> {
        Ok(())
    }

    async fn upsert_user(&self, user: User) -> Result<User, StorageError> {
        self.inner
            .write()
            .map_err(|_| StorageError::LockPoisoned)?
            .users
            .insert(user.id, user.clone());
        Ok(user)
    }

    async fn get_user(&self, user_id: Uuid) -> Result<Option<User>, StorageError> {
        Ok(self
            .inner
            .read()
            .map_err(|_| StorageError::LockPoisoned)?
            .users
            .get(&user_id)
            .cloned())
    }

    async fn upsert_linked_provider(
        &self,
        linked_provider: LinkedProvider,
    ) -> Result<LinkedProvider, StorageError> {
        self.inner
            .write()
            .map_err(|_| StorageError::LockPoisoned)?
            .linked_providers
            .insert(linked_provider.id, linked_provider.clone());
        Ok(linked_provider)
    }

    async fn list_linked_providers(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<LinkedProvider>, StorageError> {
        Ok(self
            .inner
            .read()
            .map_err(|_| StorageError::LockPoisoned)?
            .linked_providers
            .values()
            .filter(|provider| provider.user_id == user_id)
            .cloned()
            .collect())
    }

    async fn create_session(&self, session: Session) -> Result<Session, StorageError> {
        self.inner
            .write()
            .map_err(|_| StorageError::LockPoisoned)?
            .sessions
            .insert(session.id, session.clone());
        Ok(session)
    }

    async fn get_session_by_token_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<Session>, StorageError> {
        Ok(self
            .inner
            .read()
            .map_err(|_| StorageError::LockPoisoned)?
            .sessions
            .values()
            .find(|session| session.token_hash == token_hash)
            .cloned())
    }

    async fn revoke_session(&self, session_id: Uuid) -> Result<(), StorageError> {
        if let Some(session) = self
            .inner
            .write()
            .map_err(|_| StorageError::LockPoisoned)?
            .sessions
            .get_mut(&session_id)
        {
            session.revoked_at = Some(chrono::Utc::now());
        }
        Ok(())
    }

    async fn create_oauth_state(&self, state: OAuthState) -> Result<OAuthState, StorageError> {
        self.inner
            .write()
            .map_err(|_| StorageError::LockPoisoned)?
            .oauth_states
            .insert(state.state.clone(), state.clone());
        Ok(state)
    }

    async fn consume_oauth_state(
        &self,
        state: &str,
        provider: AuthProvider,
    ) -> Result<Option<OAuthState>, StorageError> {
        let mut guard = self.inner.write().map_err(|_| StorageError::LockPoisoned)?;
        let Some(existing) = guard.oauth_states.get_mut(state) else {
            return Ok(None);
        };
        if existing.provider != provider || existing.consumed_at.is_some() {
            return Ok(None);
        }
        existing.consumed_at = Some(chrono::Utc::now());
        Ok(Some(existing.clone()))
    }

    async fn find_linked_provider(
        &self,
        provider: AuthProvider,
        provider_user_id: &str,
    ) -> Result<Option<LinkedProvider>, StorageError> {
        Ok(self
            .inner
            .read()
            .map_err(|_| StorageError::LockPoisoned)?
            .linked_providers
            .values()
            .find(|linked| {
                linked.provider == provider && linked.provider_user_id == provider_user_id
            })
            .cloned())
    }

    async fn list_library(&self, user_id: Uuid) -> Result<Vec<AnimeListEntry>, StorageError> {
        Ok(self
            .inner
            .read()
            .map_err(|_| StorageError::LockPoisoned)?
            .library
            .values()
            .filter(|entry| entry.user_id == user_id)
            .cloned()
            .collect())
    }

    async fn upsert_library_entry(
        &self,
        entry: AnimeListEntry,
    ) -> Result<AnimeListEntry, StorageError> {
        self.inner
            .write()
            .map_err(|_| StorageError::LockPoisoned)?
            .library
            .insert(entry.id, entry.clone());
        Ok(entry)
    }

    async fn upsert_watch_progress(
        &self,
        progress: WatchProgress,
    ) -> Result<WatchProgress, StorageError> {
        self.inner
            .write()
            .map_err(|_| StorageError::LockPoisoned)?
            .progress
            .insert(progress.id, progress.clone());
        Ok(progress)
    }

    async fn list_watch_progress(&self, user_id: Uuid) -> Result<Vec<WatchProgress>, StorageError> {
        Ok(self
            .inner
            .read()
            .map_err(|_| StorageError::LockPoisoned)?
            .progress
            .values()
            .filter(|progress| progress.user_id == user_id)
            .cloned()
            .collect())
    }

    async fn register_addon(
        &self,
        addon: AddonRegistration,
    ) -> Result<AddonRegistration, StorageError> {
        self.inner
            .write()
            .map_err(|_| StorageError::LockPoisoned)?
            .addons
            .insert(addon.id, addon.clone());
        Ok(addon)
    }

    async fn update_addon(
        &self,
        addon: AddonRegistration,
    ) -> Result<AddonRegistration, StorageError> {
        self.inner
            .write()
            .map_err(|_| StorageError::LockPoisoned)?
            .addons
            .insert(addon.id, addon.clone());
        Ok(addon)
    }

    async fn delete_addon(&self, addon_id: Uuid) -> Result<(), StorageError> {
        self.inner
            .write()
            .map_err(|_| StorageError::LockPoisoned)?
            .addons
            .remove(&addon_id);
        Ok(())
    }

    async fn list_addons(&self) -> Result<Vec<AddonRegistration>, StorageError> {
        Ok(self
            .inner
            .read()
            .map_err(|_| StorageError::LockPoisoned)?
            .addons
            .values()
            .cloned()
            .collect())
    }

    async fn get_metadata_cache(
        &self,
        addon_id: Uuid,
        cache_key: &str,
    ) -> Result<Option<MetadataCacheEntry>, StorageError> {
        Ok(self
            .inner
            .read()
            .map_err(|_| StorageError::LockPoisoned)?
            .metadata_cache
            .get(&(addon_id, cache_key.to_owned()))
            .cloned())
    }

    async fn set_metadata_cache(
        &self,
        entry: MetadataCacheEntry,
    ) -> Result<MetadataCacheEntry, StorageError> {
        self.inner
            .write()
            .map_err(|_| StorageError::LockPoisoned)?
            .metadata_cache
            .insert((entry.addon_id, entry.cache_key.clone()), entry.clone());
        Ok(entry)
    }
}
