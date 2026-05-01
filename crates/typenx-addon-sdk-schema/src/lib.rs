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
            resources: vec![AddonResource::Catalog, AddonResource::Search],
            catalogs: vec![CatalogDefinition {
                id: "popular".to_owned(),
                name: "Popular".to_owned(),
                content_type: ContentType::Anime,
                filters: vec![],
            }],
        };

        let json = serde_json::to_string(&manifest).unwrap();
        assert!(json.contains("\"catalog\""));
        assert!(json.contains("\"anime\""));
    }
}
