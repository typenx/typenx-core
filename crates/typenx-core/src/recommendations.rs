use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
};

use serde::{Deserialize, Serialize};
use typenx_addon_sdk_schema::{
    AnimeMetadata, AnimePreview, CatalogRequest, CatalogResponse, ContentType, RecommendationItem,
    RecommendationResponse,
};
use utoipa::ToSchema;

use crate::library::{AnimeListEntry, WatchProgress, WatchStatus};

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
pub struct TypenxRecommendationRequest {
    pub addon_id: Option<String>,
    pub limit: Option<u32>,
    pub candidate_limit: Option<u32>,
    pub include_reasons: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct PrecomputedRecommendationArtifact {
    pub version: u32,
    pub generated_at: String,
    pub backend: String,
    pub users: HashMap<String, Vec<RecommendationItem>>,
}

pub fn read_precomputed_recommendations(
    path: &Path,
    user_id: &str,
    limit: u32,
) -> Option<RecommendationResponse> {
    let artifact = fs::read_to_string(path)
        .ok()
        .and_then(|body| serde_json::from_str::<PrecomputedRecommendationArtifact>(&body).ok())?;
    let mut items = artifact.users.get(user_id)?.clone();
    items.truncate(limit as usize);
    Some(RecommendationResponse { items })
}

#[derive(Clone, Debug, PartialEq)]
pub struct TasteSeed {
    pub provider_anime_id: String,
    pub title: String,
    pub score: Option<f32>,
    pub status: WatchStatus,
    pub progress_episodes: u32,
    pub total_episodes: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct TasteProfile {
    weights: HashMap<String, f32>,
    liked_titles: HashSet<String>,
}

impl TasteProfile {
    pub fn from_user_data(library: &[AnimeListEntry], progress: &[WatchProgress]) -> Self {
        let mut weights = HashMap::new();
        let mut liked_titles = HashSet::new();

        for entry in library {
            let weight = library_weight(entry);
            if weight > 0.0 {
                liked_titles.insert(normalize(&entry.title));
            }
            add_feature(
                &mut weights,
                format!("title:{}", normalize(&entry.title)),
                weight,
            );
            add_feature(
                &mut weights,
                format!("status:{:?}", entry.status),
                weight * 0.4,
            );
            if let Some(score) = entry.score {
                add_feature(
                    &mut weights,
                    format!("score_bucket:{}", score_bucket(score)),
                    weight * 0.35,
                );
            }
        }

        for item in progress {
            let weight = if item.completed { 0.75 } else { 0.2 };
            add_feature(
                &mut weights,
                format!("watched:{}", normalize(&item.anime_id)),
                weight,
            );
        }

        Self {
            weights,
            liked_titles,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.weights.is_empty()
    }
}

pub fn default_candidate_requests(limit: u32, addon_id: Option<String>) -> Vec<CatalogRequest> {
    ["popular", "trending", "highest-rated", "airing"]
        .into_iter()
        .map(|catalog_id| CatalogRequest {
            addon_id: addon_id.clone(),
            catalog_id: catalog_id.to_owned(),
            skip: Some(0),
            limit: Some(limit),
            query: None,
        })
        .collect()
}

pub fn rank_recommendations(
    profile: &TasteProfile,
    library: &[AnimeListEntry],
    candidates: Vec<CatalogResponse>,
    metadata: &HashMap<String, AnimeMetadata>,
    limit: u32,
    include_reasons: bool,
) -> RecommendationResponse {
    let seen_ids: HashSet<_> = library
        .iter()
        .map(|entry| entry.provider_anime_id.as_str())
        .collect();
    let seen_titles: HashSet<_> = library
        .iter()
        .map(|entry| normalize(&entry.title))
        .collect();

    let mut unique = HashMap::<String, AnimePreview>::new();
    for response in candidates {
        for item in response.items {
            unique.entry(item.id.clone()).or_insert(item);
        }
    }

    let mut scored = unique
        .into_values()
        .filter(|item| !seen_ids.contains(item.id.as_str()))
        .filter(|item| !seen_titles.contains(&normalize(&item.title)))
        .map(|item| score_candidate(item, profile, metadata, include_reasons))
        .collect::<Vec<_>>();

    scored.sort_by(|a, b| {
        b.recommendation_score
            .partial_cmp(&a.recommendation_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    RecommendationResponse {
        items: diversify(scored, limit as usize),
    }
}

fn score_candidate(
    anime: AnimePreview,
    profile: &TasteProfile,
    metadata: &HashMap<String, AnimeMetadata>,
    include_reasons: bool,
) -> RecommendationItem {
    let mut features = preview_features(&anime);
    if let Some(meta) = metadata.get(&anime.id) {
        features.extend(metadata_features(meta));
    }

    let affinity = features
        .iter()
        .map(|feature| profile.weights.get(feature).copied().unwrap_or_default())
        .sum::<f32>()
        / (features.len().max(1) as f32).sqrt();
    let quality = anime.score.unwrap_or_default() / 10.0;
    let popularity_prior = if anime.poster.is_some() { 0.08 } else { 0.0 };
    let title_echo = if profile.liked_titles.contains(&normalize(&anime.title)) {
        -2.0
    } else {
        0.0
    };
    let recommendation_score =
        affinity.mul_add(0.82, quality * 0.14) + popularity_prior + title_echo;
    let reasons = if include_reasons {
        reasons_for(&anime, &features, profile)
    } else {
        vec![]
    };

    RecommendationItem {
        anime,
        recommendation_score: (recommendation_score * 10_000.0).round() / 10_000.0,
        reasons,
    }
}

fn library_weight(entry: &AnimeListEntry) -> f32 {
    let score_weight = entry.score.map(|score| (score - 5.0) / 2.5).unwrap_or(0.0);
    let status_weight = match entry.status {
        WatchStatus::Completed => 1.0,
        WatchStatus::Watching => 0.8,
        WatchStatus::Planning => 0.15,
        WatchStatus::Paused => -0.25,
        WatchStatus::Dropped => -1.25,
    };
    let progress_weight = entry
        .total_episodes
        .filter(|total| *total > 0)
        .map(|total| entry.progress_episodes as f32 / total as f32)
        .unwrap_or_default()
        .min(1.0);
    score_weight + status_weight + progress_weight * 0.5
}

fn preview_features(anime: &AnimePreview) -> Vec<String> {
    let mut features = anime
        .genres
        .iter()
        .map(|genre| format!("genre:{}", normalize(genre)))
        .collect::<Vec<_>>();
    features.push(format!("type:{}", content_type_key(&anime.content_type)));
    if let Some(year) = anime.year {
        features.push(format!("era:{}", year / 5 * 5));
    }
    features
}

fn metadata_features(anime: &AnimeMetadata) -> Vec<String> {
    anime
        .tags
        .iter()
        .map(|tag| format!("tag:{}", normalize(tag)))
        .chain(
            anime
                .studios
                .iter()
                .map(|studio| format!("studio:{}", normalize(studio))),
        )
        .chain(
            anime
                .source
                .iter()
                .map(|source| format!("source:{}", normalize(source))),
        )
        .collect()
}

fn reasons_for(anime: &AnimePreview, features: &[String], profile: &TasteProfile) -> Vec<String> {
    let mut reasons = features
        .iter()
        .filter(|feature| profile.weights.get(*feature).copied().unwrap_or_default() > 0.0)
        .take(3)
        .map(|feature| {
            feature
                .split_once(':')
                .map(|(_, value)| value.replace('-', " "))
                .unwrap_or_else(|| feature.clone())
        })
        .collect::<Vec<_>>();
    if anime.score.is_some_and(|score| score >= 8.0) {
        reasons.push("high community score".to_owned());
    }
    reasons
}

fn diversify(items: Vec<RecommendationItem>, limit: usize) -> Vec<RecommendationItem> {
    let mut selected = Vec::new();
    let mut genre_counts = HashMap::<String, usize>::new();
    for item in items {
        let max_genre_count = item
            .anime
            .genres
            .iter()
            .map(|genre| {
                genre_counts
                    .get(&normalize(genre))
                    .copied()
                    .unwrap_or_default()
            })
            .max()
            .unwrap_or_default();
        if max_genre_count < 5 || selected.len() < limit / 3 {
            for genre in &item.anime.genres {
                *genre_counts.entry(normalize(genre)).or_default() += 1;
            }
            selected.push(item);
        }
        if selected.len() >= limit {
            break;
        }
    }
    selected
}

fn add_feature(weights: &mut HashMap<String, f32>, feature: String, weight: f32) {
    *weights.entry(feature).or_default() += weight;
}

fn score_bucket(score: f32) -> &'static str {
    if score >= 8.0 {
        "loved"
    } else if score >= 6.0 {
        "liked"
    } else if score >= 4.0 {
        "mixed"
    } else {
        "disliked"
    }
}

fn content_type_key(content_type: &ContentType) -> &'static str {
    match content_type {
        ContentType::Anime => "anime",
        ContentType::Movie => "movie",
        ContentType::Ova => "ova",
        ContentType::Ona => "ona",
        ContentType::Special => "special",
    }
}

fn normalize(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_owned()
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use uuid::Uuid;

    use super::*;
    use crate::auth::AuthProvider;

    #[test]
    fn dropped_low_score_entries_push_recommendations_down() {
        let library = vec![AnimeListEntry {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            provider: AuthProvider::AniList,
            provider_anime_id: "1".to_owned(),
            title: "Space War".to_owned(),
            status: WatchStatus::Dropped,
            score: Some(2.0),
            progress_episodes: 1,
            total_episodes: Some(12),
            updated_at: Utc::now(),
        }];
        let profile = TasteProfile::from_user_data(&library, &[]);
        assert!(
            profile
                .weights
                .get("title:space-war")
                .copied()
                .unwrap_or_default()
                < 0.0
        );
    }

    #[test]
    fn reads_precomputed_user_recommendations() {
        let path = std::env::temp_dir().join(format!("typenx-rec-{}.json", Uuid::new_v4()));
        std::fs::write(
            &path,
            r#"{
              "version": 1,
              "generated_at": "2026-05-02T00:00:00Z",
              "backend": "directml",
              "users": {
                "user-1": [{
                  "id": "anime-1",
                  "title": "Test Anime",
                  "poster": null,
                  "banner": null,
                  "synopsis": null,
                  "score": null,
                  "year": null,
                  "content_type": "anime",
                  "genres": [],
                  "season_entries": [],
                  "recommendation_score": 0.91,
                  "reasons": ["gpu trained implicit feedback"]
                }]
              }
            }"#,
        )
        .unwrap();

        let response = read_precomputed_recommendations(&path, "user-1", 10).unwrap();
        std::fs::remove_file(path).unwrap();

        assert_eq!(response.items.len(), 1);
        assert_eq!(response.items[0].anime.title, "Test Anime");
        assert_eq!(
            response.items[0].reasons,
            vec!["gpu trained implicit feedback"]
        );
    }
}
