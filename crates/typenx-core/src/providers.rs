use async_trait::async_trait;
use thiserror::Error;

use crate::{
    auth::{AuthProvider, ProviderIdentity},
    library::ProviderListSync,
};

#[async_trait]
pub trait AnimeProviderClient: Send + Sync {
    fn provider(&self) -> AuthProvider;
    fn authorization_url(&self, state: &str) -> String;
    async fn exchange_code(&self, code: &str) -> Result<ProviderIdentity, ProviderError>;
    async fn sync_list(
        &self,
        identity: &ProviderIdentity,
    ) -> Result<ProviderListSync, ProviderError>;
}

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("provider rejected the oauth code")]
    InvalidCode,
    #[error("provider request failed: {0}")]
    Request(String),
    #[error("provider returned invalid data: {0}")]
    InvalidData(String),
}

#[derive(Clone, Debug)]
pub struct ProviderSyncJob {
    pub provider: AuthProvider,
    pub provider_user_id: String,
}
