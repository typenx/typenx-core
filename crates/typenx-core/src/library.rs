use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::auth::AuthProvider;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WatchStatus {
    Planning,
    Watching,
    Completed,
    Paused,
    Dropped,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema, PartialEq)]
pub struct AnimeListEntry {
    pub id: Uuid,
    pub user_id: Uuid,
    pub provider: AuthProvider,
    pub provider_anime_id: String,
    pub title: String,
    pub status: WatchStatus,
    pub score: Option<f32>,
    pub progress_episodes: u32,
    pub total_episodes: Option<u32>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
pub struct WatchProgress {
    pub id: Uuid,
    pub user_id: Uuid,
    pub anime_id: String,
    pub episode_id: Option<String>,
    pub episode_number: Option<u32>,
    pub position_seconds: u32,
    pub duration_seconds: Option<u32>,
    pub completed: bool,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema, PartialEq)]
pub struct ProviderListSync {
    pub provider: AuthProvider,
    pub entries: Vec<AnimeListEntry>,
    pub synced_at: DateTime<Utc>,
}
