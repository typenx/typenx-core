use async_trait::async_trait;
use thiserror::Error;

use crate::{addons::AddonRegistration, providers::ProviderSyncJob};

#[async_trait]
pub trait JobRunner: Send + Sync {
    async fn sync_provider_list(&self, job: ProviderSyncJob) -> Result<(), JobError>;
    async fn refresh_addon_cache(&self, addon: AddonRegistration) -> Result<(), JobError>;
}

#[derive(Debug, Error)]
pub enum JobError {
    #[error("job failed: {0}")]
    Failed(String),
}
