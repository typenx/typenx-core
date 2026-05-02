use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
pub struct AddonManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub resources: Vec<AddonResource>,
    pub catalogs: Vec<CatalogDefinition>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AddonResource {
    Catalog,
    Search,
    AnimeMeta,
    EpisodeMeta,
    VideoSources,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
pub struct CatalogDefinition {
    pub id: String,
    pub name: String,
    pub content_type: ContentType,
    pub filters: Vec<CatalogFilter>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContentType {
    Anime,
    Movie,
    Ova,
    Ona,
    Special,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
pub struct CatalogFilter {
    pub id: String,
    pub name: String,
    pub values: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
pub struct CatalogRequest {
    pub addon_id: Option<String>,
    pub catalog_id: String,
    pub skip: Option<u32>,
    pub limit: Option<u32>,
    pub query: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
pub struct SearchRequest {
    pub addon_id: Option<String>,
    pub query: String,
    pub limit: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
pub struct VideoSourceRequest {
    pub addon_id: Option<String>,
    pub anime_id: String,
    pub anime_title: Option<String>,
    pub episode_id: Option<String>,
    pub episode_title: Option<String>,
    pub episode_number: Option<u32>,
    pub season_number: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
pub struct VideoSourceResponse {
    pub streams: Vec<VideoStream>,
    #[serde(default)]
    pub subtitles: Vec<VideoSubtitle>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
pub struct VideoStream {
    pub id: String,
    pub title: Option<String>,
    pub url: String,
    pub quality: Option<String>,
    pub format: Option<String>,
    pub audio_language: Option<String>,
    pub headers: Vec<VideoHeader>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
pub struct VideoHeader {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
pub struct VideoSubtitle {
    pub id: String,
    pub label: String,
    pub language: Option<String>,
    pub url: String,
    pub format: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema, PartialEq)]
pub struct CatalogResponse {
    pub items: Vec<AnimePreview>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema, PartialEq)]
pub struct AnimePreview {
    pub id: String,
    pub title: String,
    pub poster: Option<String>,
    pub banner: Option<String>,
    pub synopsis: Option<String>,
    pub score: Option<f32>,
    pub year: Option<i32>,
    pub content_type: ContentType,
    #[serde(default)]
    pub genres: Vec<String>,
    #[serde(default)]
    pub season_entries: Vec<SeasonEntry>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
pub struct SeasonEntry {
    pub id: String,
    pub title: String,
    pub season_number: Option<u32>,
    pub year: Option<i32>,
    pub episode_count: Option<u32>,
    pub source: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema, PartialEq)]
pub struct AnimeMetadata {
    pub id: String,
    pub title: String,
    pub original_title: Option<String>,
    #[serde(default)]
    pub alternative_titles: Vec<String>,
    pub synopsis: Option<String>,
    pub description: Option<String>,
    pub poster: Option<String>,
    pub banner: Option<String>,
    pub year: Option<i32>,
    pub season: Option<String>,
    pub season_year: Option<i32>,
    pub status: Option<String>,
    pub content_type: ContentType,
    pub source: Option<String>,
    pub duration_minutes: Option<u32>,
    pub episode_count: Option<u32>,
    pub score: Option<f32>,
    pub rank: Option<u32>,
    pub popularity: Option<u32>,
    pub rating: Option<String>,
    #[serde(default)]
    pub genres: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub studios: Vec<String>,
    #[serde(default)]
    pub staff: Vec<StaffCredit>,
    pub country_of_origin: Option<String>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub site_url: Option<String>,
    pub trailer_url: Option<String>,
    #[serde(default)]
    pub external_links: Vec<ExternalLink>,
    #[serde(default)]
    pub episodes: Vec<EpisodeMetadata>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
pub struct StaffCredit {
    pub name: String,
    pub role: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
pub struct ExternalLink {
    pub site: String,
    pub url: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
pub struct EpisodeMetadata {
    pub id: String,
    pub anime_id: String,
    pub season_number: Option<u32>,
    pub number: u32,
    pub title: Option<String>,
    pub synopsis: Option<String>,
    pub thumbnail: Option<String>,
    pub duration_minutes: Option<u32>,
    pub source: Option<String>,
    pub aired_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
pub struct AddonHealth {
    pub ok: bool,
    pub message: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addon_manifest_serializes_protocol_shape() {
        let manifest = AddonManifest {
            id: "schema-test-addon".to_owned(),
            name: "Schema Test Addon".to_owned(),
            version: "0.1.0".to_owned(),
            description: None,
            icon: Some("https://typenx.dev/addon-icon.png".to_owned()),
            resources: vec![
                AddonResource::Catalog,
                AddonResource::Search,
                AddonResource::AnimeMeta,
                AddonResource::VideoSources,
            ],
            catalogs: vec![CatalogDefinition {
                id: "popular".to_owned(),
                name: "Popular".to_owned(),
                content_type: ContentType::Anime,
                filters: vec![],
            }],
        };

        let json = serde_json::to_string(&manifest).unwrap();
        assert!(json.contains("\"catalog\""));
        assert!(json.contains("\"anime_meta\""));
        assert!(json.contains("\"video_sources\""));
    }

    #[test]
    fn anime_preview_preserves_season_entries() {
        let preview = AnimePreview {
            id: "central:aot".to_owned(),
            title: "Attack on Titan".to_owned(),
            poster: None,
            banner: None,
            synopsis: None,
            score: None,
            year: Some(2013),
            content_type: ContentType::Anime,
            genres: vec!["Action".to_owned(), "Drama".to_owned()],
            season_entries: vec![SeasonEntry {
                id: "aot-s2".to_owned(),
                title: "Attack on Titan Season 2".to_owned(),
                season_number: Some(2),
                year: Some(2017),
                episode_count: Some(12),
                source: Some("Kitsu".to_owned()),
            }],
        };

        let json = serde_json::to_string(&preview).unwrap();
        let roundtrip: AnimePreview = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtrip.genres, vec!["Action", "Drama"]);
        assert_eq!(roundtrip.season_entries[0].season_number, Some(2));
    }

    #[test]
    fn video_sources_serialize_protocol_shape() {
        let response = VideoSourceResponse {
            streams: vec![VideoStream {
                id: "main-1080p".to_owned(),
                title: Some("Main".to_owned()),
                url: "https://cdn.example/anime/episode-1.m3u8".to_owned(),
                quality: Some("1080p".to_owned()),
                format: Some("hls".to_owned()),
                audio_language: Some("ja".to_owned()),
                headers: vec![VideoHeader {
                    name: "referer".to_owned(),
                    value: "https://example.test".to_owned(),
                }],
            }],
            subtitles: vec![VideoSubtitle {
                id: "en".to_owned(),
                label: "English".to_owned(),
                language: Some("en".to_owned()),
                url: "https://cdn.example/subs/en.vtt".to_owned(),
                format: Some("vtt".to_owned()),
            }],
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"streams\""));
        assert!(json.contains("\"subtitles\""));
    }
}
