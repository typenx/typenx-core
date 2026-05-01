use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{get, post},
    Router,
};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use typenx_addon_sdk_schema::{
    AddonManifest, AnimeMetadata, CatalogRequest, CatalogResponse, SearchRequest,
};
use typenx_core::{
    addons::{AddonRegistration, RegisterAddonRequest, RemoteAddonClient},
    auth::{AuthProvider, LinkedProvider, LoginResult, ProviderIdentity, Session, User},
};
use typenx_storage::TypenxStore;
use utoipa::{IntoParams, OpenApi, ToSchema};
use uuid::Uuid;

#[derive(Clone)]
pub struct AppState {
    store: Arc<dyn TypenxStore>,
    addon_client: RemoteAddonClient,
}

impl AppState {
    pub fn new(store: Arc<dyn TypenxStore>) -> Self {
        Self {
            store,
            addon_client: RemoteAddonClient::new(),
        }
    }
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/openapi.json", get(openapi))
        .route("/auth/anilist/login", get(auth_anilist_login))
        .route("/auth/anilist/callback", get(auth_anilist_callback))
        .route("/auth/mal/login", get(auth_mal_login))
        .route("/auth/mal/callback", get(auth_mal_callback))
        .route("/me", get(me))
        .route("/me/providers", get(me_providers))
        .route("/me/library", get(me_library))
        .route("/me/progress", get(me_progress))
        .route("/addons", get(list_addons).post(register_addon))
        .route("/addons/{id}/manifest", get(addon_manifest))
        .route("/catalogs", post(catalogs))
        .route("/search", post(search))
        .route("/anime/{id}", get(anime_meta))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

#[derive(OpenApi)]
#[openapi(
    paths(
        health,
        openapi,
        auth_anilist_login,
        auth_anilist_callback,
        auth_mal_login,
        auth_mal_callback,
        me,
        me_providers,
        me_library,
        me_progress,
        list_addons,
        register_addon,
        addon_manifest,
        catalogs,
        search,
        anime_meta
    ),
    components(schemas(
        ApiError,
        HealthResponse,
        OAuthLoginResponse,
        OAuthCallbackQuery,
        LoginResult,
        ProviderIdentity,
        User,
        LinkedProvider,
        Session,
        RegisterAddonRequest,
        AddonRegistration,
        AddonManifest,
        CatalogRequest,
        SearchRequest,
        CatalogResponse,
        AnimeMetadata
    ))
)]
pub struct ApiDoc;

#[derive(Debug, Serialize, ToSchema)]
pub struct HealthResponse {
    pub ok: bool,
    pub service: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct OAuthLoginResponse {
    pub provider: AuthProvider,
    pub authorization_url: String,
}

#[derive(Debug, Deserialize, IntoParams, ToSchema)]
pub struct OAuthCallbackQuery {
    pub code: String,
    pub state: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ApiError {
    pub message: String,
}

#[utoipa::path(get, path = "/health", responses((status = 200, body = HealthResponse)))]
async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        ok: true,
        service: "typenx-server".to_owned(),
    })
}

#[utoipa::path(get, path = "/openapi.json", responses((status = 200)))]
async fn openapi() -> Json<utoipa::openapi::OpenApi> {
    Json(ApiDoc::openapi())
}

#[utoipa::path(get, path = "/auth/anilist/login", responses((status = 200, body = OAuthLoginResponse)))]
async fn auth_anilist_login() -> Json<OAuthLoginResponse> {
    Json(oauth_login_response(AuthProvider::AniList))
}

#[utoipa::path(get, path = "/auth/mal/login", responses((status = 200, body = OAuthLoginResponse)))]
async fn auth_mal_login() -> Json<OAuthLoginResponse> {
    Json(oauth_login_response(AuthProvider::MyAnimeList))
}

#[utoipa::path(
    get,
    path = "/auth/anilist/callback",
    params(OAuthCallbackQuery),
    responses((status = 200, body = LoginResult))
)]
async fn auth_anilist_callback(
    State(state): State<AppState>,
    Query(query): Query<OAuthCallbackQuery>,
) -> Result<Json<LoginResult>, ApiFailure> {
    mocked_provider_login(state, AuthProvider::AniList, query.code)
        .await
        .map(Json)
}

#[utoipa::path(
    get,
    path = "/auth/mal/callback",
    params(OAuthCallbackQuery),
    responses((status = 200, body = LoginResult))
)]
async fn auth_mal_callback(
    State(state): State<AppState>,
    Query(query): Query<OAuthCallbackQuery>,
) -> Result<Json<LoginResult>, ApiFailure> {
    mocked_provider_login(state, AuthProvider::MyAnimeList, query.code)
        .await
        .map(Json)
}

#[utoipa::path(get, path = "/me", responses((status = 501, body = ApiError)))]
async fn me() -> ApiFailure {
    ApiFailure::not_implemented("session extraction is planned after the storage/API skeleton")
}

#[utoipa::path(get, path = "/me/providers", responses((status = 501, body = ApiError)))]
async fn me_providers() -> ApiFailure {
    ApiFailure::not_implemented("session extraction is planned after the storage/API skeleton")
}

#[utoipa::path(get, path = "/me/library", responses((status = 501, body = ApiError)))]
async fn me_library() -> ApiFailure {
    ApiFailure::not_implemented("provider list sync is planned after OAuth clients are wired")
}

#[utoipa::path(get, path = "/me/progress", responses((status = 501, body = ApiError)))]
async fn me_progress() -> ApiFailure {
    ApiFailure::not_implemented(
        "watch progress APIs are represented in storage but not exposed yet",
    )
}

#[utoipa::path(get, path = "/addons", responses((status = 200, body = Vec<AddonRegistration>)))]
async fn list_addons(
    State(state): State<AppState>,
) -> Result<Json<Vec<AddonRegistration>>, ApiFailure> {
    Ok(Json(state.store.list_addons().await?))
}

#[utoipa::path(
    post,
    path = "/addons",
    request_body = RegisterAddonRequest,
    responses((status = 200, body = AddonRegistration))
)]
async fn register_addon(
    State(state): State<AppState>,
    Json(request): Json<RegisterAddonRequest>,
) -> Result<Json<AddonRegistration>, ApiFailure> {
    let manifest = state.addon_client.manifest(&request.base_url).await?;
    let now = Utc::now();
    let addon = AddonRegistration {
        id: Uuid::new_v4(),
        base_url: request.base_url,
        enabled: true,
        manifest: Some(manifest),
        created_at: now,
        updated_at: now,
    };
    Ok(Json(state.store.register_addon(addon).await?))
}

#[utoipa::path(
    get,
    path = "/addons/{id}/manifest",
    params(("id" = Uuid, Path, description = "Addon id")),
    responses((status = 200, body = AddonManifest), (status = 404, body = ApiError))
)]
async fn addon_manifest(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<AddonManifest>, ApiFailure> {
    let addon = state
        .store
        .list_addons()
        .await?
        .into_iter()
        .find(|addon| addon.id == id)
        .ok_or_else(|| ApiFailure::not_found("addon not found"))?;
    addon
        .manifest
        .map(Json)
        .ok_or_else(|| ApiFailure::not_found("addon has no manifest cached"))
}

#[utoipa::path(
    post,
    path = "/catalogs",
    request_body = CatalogRequest,
    responses((status = 200, body = CatalogResponse))
)]
async fn catalogs(
    State(state): State<AppState>,
    Json(request): Json<CatalogRequest>,
) -> Result<Json<CatalogResponse>, ApiFailure> {
    let addon = first_enabled_addon(&state).await?;
    Ok(Json(
        state
            .addon_client
            .catalog(&addon.base_url, &request)
            .await?,
    ))
}

#[utoipa::path(
    post,
    path = "/search",
    request_body = SearchRequest,
    responses((status = 200, body = CatalogResponse))
)]
async fn search(
    State(state): State<AppState>,
    Json(request): Json<SearchRequest>,
) -> Result<Json<CatalogResponse>, ApiFailure> {
    let addon = first_enabled_addon(&state).await?;
    Ok(Json(
        state.addon_client.search(&addon.base_url, &request).await?,
    ))
}

#[utoipa::path(
    get,
    path = "/anime/{id}",
    params(("id" = String, Path, description = "Addon anime id")),
    responses((status = 200, body = AnimeMetadata))
)]
async fn anime_meta(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<AnimeMetadata>, ApiFailure> {
    let addon = first_enabled_addon(&state).await?;
    Ok(Json(
        state.addon_client.anime_meta(&addon.base_url, &id).await?,
    ))
}

fn oauth_login_response(provider: AuthProvider) -> OAuthLoginResponse {
    OAuthLoginResponse {
        provider,
        authorization_url: format!(
            "https://typenx.local/auth/{}/authorize-placeholder",
            provider.as_str()
        ),
    }
}

async fn mocked_provider_login(
    state: AppState,
    provider: AuthProvider,
    code: String,
) -> Result<LoginResult, ApiFailure> {
    let now = Utc::now();
    let user = User {
        id: Uuid::new_v4(),
        display_name: format!("typenx-{code}"),
        avatar_url: None,
        created_at: now,
        updated_at: now,
    };
    let linked_provider = LinkedProvider {
        id: Uuid::new_v4(),
        user_id: user.id,
        provider,
        provider_user_id: code.clone(),
        provider_username: format!("provider-{code}"),
        access_token: "mock-access-token".to_owned(),
        refresh_token: None,
        expires_at: None,
        linked_at: now,
    };
    let session_token = Uuid::new_v4().to_string();
    let session = Session {
        id: Uuid::new_v4(),
        user_id: user.id,
        token_hash: session_token.clone(),
        created_at: now,
        expires_at: now + Duration::days(30),
        revoked_at: None,
    };

    state.store.upsert_user(user.clone()).await?;
    state
        .store
        .upsert_linked_provider(linked_provider.clone())
        .await?;
    state.store.create_session(session.clone()).await?;

    Ok(LoginResult {
        user,
        linked_provider,
        session,
        session_token,
    })
}

async fn first_enabled_addon(state: &AppState) -> Result<AddonRegistration, ApiFailure> {
    state
        .store
        .list_addons()
        .await?
        .into_iter()
        .find(|addon| addon.enabled)
        .ok_or_else(|| ApiFailure::not_found("no enabled addon registered"))
}

#[derive(Debug)]
pub struct ApiFailure {
    status: StatusCode,
    message: String,
}

impl ApiFailure {
    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    fn not_implemented(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_IMPLEMENTED,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiFailure {
    fn into_response(self) -> axum::response::Response {
        (
            self.status,
            Json(ApiError {
                message: self.message,
            }),
        )
            .into_response()
    }
}

impl From<typenx_storage::StorageError> for ApiFailure {
    fn from(error: typenx_storage::StorageError) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: error.to_string(),
        }
    }
}

impl From<typenx_core::addons::AddonClientError> for ApiFailure {
    fn from(error: typenx_core::addons::AddonClientError) -> Self {
        Self {
            status: StatusCode::BAD_GATEWAY,
            message: error.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;
    use typenx_storage::memory::MemoryStore;

    #[tokio::test]
    async fn openapi_endpoint_returns_schema() {
        let state = AppState::new(Arc::new(MemoryStore::default()));
        let response = build_router(state)
            .oneshot(Request::get("/openapi.json").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn mocked_oauth_callback_creates_login_result() {
        let state = AppState::new(Arc::new(MemoryStore::default()));
        let response = build_router(state)
            .oneshot(
                Request::get("/auth/anilist/callback?code=test-user")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}
