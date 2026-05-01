use std::{collections::HashMap, env, sync::Arc};

use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Json, Redirect, Response},
    routing::{get, post},
    Router,
};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    trace::TraceLayer,
};
use typenx_addon_sdk_schema::{
    AddonManifest, AnimeMetadata, CatalogRequest, CatalogResponse, SearchRequest,
};
use typenx_core::{
    addons::{AddonRegistration, MetadataCacheEntry, RegisterAddonRequest, RemoteAddonClient},
    auth::{AuthProvider, CurrentUser, LinkedProvider, LoginResult, OAuthState, Session, User},
    providers::{
        new_mal_pkce_verifier, AniListClient, AnimeProviderClient, MyAnimeListClient,
        OAuthProviderConfig,
    },
    security::{hash_token, protect_token, random_url_token},
};
use typenx_storage::TypenxStore;
use utoipa::{IntoParams, OpenApi, ToSchema};
use uuid::Uuid;

const SESSION_COOKIE: &str = "typenx_session";

#[derive(Clone)]
pub struct AppConfig {
    pub public_base_url: String,
    pub web_redirect_url: String,
    pub session_secret: String,
    pub secure_cookies: bool,
}

impl AppConfig {
    pub fn from_env() -> Self {
        let public_base_url = env::var("TYPENX_PUBLIC_BASE_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:8080".to_owned());
        Self {
            secure_cookies: public_base_url.starts_with("https://"),
            public_base_url,
            web_redirect_url: env::var("TYPENX_WEB_REDIRECT_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:3000".to_owned()),
            session_secret: env::var("TYPENX_SESSION_SECRET")
                .unwrap_or_else(|_| "typenx-dev-session-secret-change-me".to_owned()),
        }
    }

    fn callback_url(&self, provider: AuthProvider) -> String {
        let provider_path = match provider {
            AuthProvider::AniList => "anilist",
            AuthProvider::MyAnimeList => "mal",
        };
        format!(
            "{}/auth/{provider_path}/callback",
            self.public_base_url.trim_end_matches('/')
        )
    }
}

#[derive(Clone)]
pub struct AppState {
    store: Arc<dyn TypenxStore>,
    addon_client: RemoteAddonClient,
    config: AppConfig,
    providers: HashMap<AuthProvider, Arc<dyn AnimeProviderClient>>,
}

impl AppState {
    pub fn new(store: Arc<dyn TypenxStore>) -> Self {
        Self::from_config(store, AppConfig::from_env())
    }

    pub fn from_config(store: Arc<dyn TypenxStore>, config: AppConfig) -> Self {
        let mut providers: HashMap<AuthProvider, Arc<dyn AnimeProviderClient>> = HashMap::new();
        if let (Ok(client_id), Ok(client_secret)) = (
            env::var("ANILIST_CLIENT_ID"),
            env::var("ANILIST_CLIENT_SECRET"),
        ) {
            providers.insert(
                AuthProvider::AniList,
                Arc::new(AniListClient::new(OAuthProviderConfig {
                    client_id,
                    client_secret,
                    redirect_uri: config.callback_url(AuthProvider::AniList),
                })),
            );
        }
        if let (Ok(client_id), Ok(client_secret)) =
            (env::var("MAL_CLIENT_ID"), env::var("MAL_CLIENT_SECRET"))
        {
            providers.insert(
                AuthProvider::MyAnimeList,
                Arc::new(MyAnimeListClient::new(OAuthProviderConfig {
                    client_id,
                    client_secret,
                    redirect_uri: config.callback_url(AuthProvider::MyAnimeList),
                })),
            );
        }
        Self {
            store,
            addon_client: RemoteAddonClient::new(),
            config,
            providers,
        }
    }

    #[cfg(test)]
    pub fn with_provider(mut self, provider: Arc<dyn AnimeProviderClient>) -> Self {
        self.providers.insert(provider.provider(), provider);
        self
    }
}

pub fn build_router(state: AppState) -> Router {
    let cors = cors_layer(&state.config);

    Router::new()
        .route("/health", get(health))
        .route("/openapi.json", get(openapi))
        .route("/auth/anilist/login", get(auth_anilist_login))
        .route("/auth/anilist/callback", get(auth_anilist_callback))
        .route("/auth/mal/login", get(auth_mal_login))
        .route("/auth/mal/callback", get(auth_mal_callback))
        .route("/auth/logout", post(auth_logout))
        .route("/me", get(me))
        .route("/me/providers", get(me_providers))
        .route("/me/library", get(me_library))
        .route("/me/progress", get(me_progress))
        .route("/addons", get(list_addons).post(register_addon))
        .route("/addons/{id}/manifest", get(addon_manifest))
        .route("/catalogs", post(catalogs))
        .route("/search", post(search))
        .route("/anime/{id}", get(anime_meta))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

fn cors_layer(config: &AppConfig) -> CorsLayer {
    let mut allowed_origins = vec![
        "http://127.0.0.1:3000".to_owned(),
        "http://localhost:3000".to_owned(),
    ];
    if let Some(origin) = origin_from_url(&config.web_redirect_url) {
        allowed_origins.push(origin);
    }

    CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(move |origin, _| {
            origin
                .to_str()
                .is_ok_and(|origin| allowed_origins.iter().any(|allowed| allowed == origin))
        }))
        .allow_credentials(true)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([header::ACCEPT, header::CONTENT_TYPE])
}

fn origin_from_url(url: &str) -> Option<String> {
    let scheme_end = url.find("://")? + 3;
    let rest = &url[scheme_end..];
    let host_end = rest.find('/').unwrap_or(rest.len());
    Some(
        url[..scheme_end + host_end]
            .trim_end_matches('/')
            .to_owned(),
    )
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
        auth_logout,
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
        CurrentUser,
        ProviderAccount,
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

#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct OAuthLoginResponse {
    pub provider: AuthProvider,
    pub authorization_url: String,
}

#[derive(Debug, Deserialize, IntoParams, ToSchema)]
pub struct OAuthCallbackQuery {
    pub code: String,
    pub state: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ProviderAccount {
    pub id: Uuid,
    pub provider: AuthProvider,
    pub provider_user_id: String,
    pub provider_username: String,
    pub expires_at: Option<chrono::DateTime<Utc>>,
    pub linked_at: chrono::DateTime<Utc>,
}

impl From<LinkedProvider> for ProviderAccount {
    fn from(provider: LinkedProvider) -> Self {
        Self {
            id: provider.id,
            provider: provider.provider,
            provider_user_id: provider.provider_user_id,
            provider_username: provider.provider_username,
            expires_at: provider.expires_at,
            linked_at: provider.linked_at,
        }
    }
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
async fn auth_anilist_login(
    State(state): State<AppState>,
) -> Result<Json<OAuthLoginResponse>, ApiFailure> {
    login_url(state, AuthProvider::AniList).await.map(Json)
}

#[utoipa::path(get, path = "/auth/mal/login", responses((status = 200, body = OAuthLoginResponse)))]
async fn auth_mal_login(
    State(state): State<AppState>,
) -> Result<Json<OAuthLoginResponse>, ApiFailure> {
    login_url(state, AuthProvider::MyAnimeList).await.map(Json)
}

#[utoipa::path(
    get,
    path = "/auth/anilist/callback",
    params(OAuthCallbackQuery),
    responses((status = 302), (status = 400, body = ApiError))
)]
async fn auth_anilist_callback(
    State(state): State<AppState>,
    Query(query): Query<OAuthCallbackQuery>,
) -> Result<Response, ApiFailure> {
    oauth_callback(state, AuthProvider::AniList, query).await
}

#[utoipa::path(
    get,
    path = "/auth/mal/callback",
    params(OAuthCallbackQuery),
    responses((status = 302), (status = 400, body = ApiError))
)]
async fn auth_mal_callback(
    State(state): State<AppState>,
    Query(query): Query<OAuthCallbackQuery>,
) -> Result<Response, ApiFailure> {
    oauth_callback(state, AuthProvider::MyAnimeList, query).await
}

#[utoipa::path(post, path = "/auth/logout", responses((status = 204), (status = 401, body = ApiError)))]
async fn auth_logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiFailure> {
    if let Some(session) = session_from_headers(&state, &headers).await? {
        state.store.revoke_session(session.id).await?;
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&expired_session_cookie(&state.config)).expect("valid cookie"),
    );
    Ok(response)
}

#[utoipa::path(get, path = "/me", responses((status = 200, body = CurrentUser), (status = 401, body = ApiError)))]
async fn me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<CurrentUser>, ApiFailure> {
    let (user, providers) = current_user(&state, &headers).await?;
    Ok(Json(CurrentUser { user, providers }))
}

#[utoipa::path(get, path = "/me/providers", responses((status = 200, body = Vec<ProviderAccount>), (status = 401, body = ApiError)))]
async fn me_providers(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<ProviderAccount>>, ApiFailure> {
    let (_, providers) = current_user(&state, &headers).await?;
    Ok(Json(
        providers.into_iter().map(ProviderAccount::from).collect(),
    ))
}

#[utoipa::path(get, path = "/me/library", responses((status = 200), (status = 401, body = ApiError)))]
async fn me_library(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<typenx_core::library::AnimeListEntry>>, ApiFailure> {
    let (user, _) = current_user(&state, &headers).await?;
    Ok(Json(state.store.list_library(user.id).await?))
}

#[utoipa::path(get, path = "/me/progress", responses((status = 200), (status = 401, body = ApiError)))]
async fn me_progress(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<typenx_core::library::WatchProgress>>, ApiFailure> {
    let (user, _) = current_user(&state, &headers).await?;
    Ok(Json(state.store.list_watch_progress(user.id).await?))
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
    let cache_key = format!(
        "catalog:{}",
        serde_json::to_string(&request).unwrap_or_default()
    );
    if let Some(cached) = read_cache::<CatalogResponse>(&state, addon.id, &cache_key).await? {
        return Ok(Json(cached));
    }
    let response = state
        .addon_client
        .catalog(&addon.base_url, &request)
        .await?;
    write_cache(&state, addon.id, cache_key, &response).await?;
    Ok(Json(response))
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
    let cache_key = format!(
        "search:{}",
        serde_json::to_string(&request).unwrap_or_default()
    );
    if let Some(cached) = read_cache::<CatalogResponse>(&state, addon.id, &cache_key).await? {
        return Ok(Json(cached));
    }
    let response = state.addon_client.search(&addon.base_url, &request).await?;
    write_cache(&state, addon.id, cache_key, &response).await?;
    Ok(Json(response))
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
    let cache_key = format!("anime:{id}");
    if let Some(cached) = read_cache::<AnimeMetadata>(&state, addon.id, &cache_key).await? {
        return Ok(Json(cached));
    }
    let response = state.addon_client.anime_meta(&addon.base_url, &id).await?;
    write_cache(&state, addon.id, cache_key, &response).await?;
    Ok(Json(response))
}

async fn login_url(
    state: AppState,
    provider: AuthProvider,
) -> Result<OAuthLoginResponse, ApiFailure> {
    let provider_client = state
        .providers
        .get(&provider)
        .ok_or_else(|| ApiFailure::not_configured(provider))?;
    let now = Utc::now();
    let state_token = random_url_token(32);
    let pkce_verifier = (provider == AuthProvider::MyAnimeList).then(new_mal_pkce_verifier);
    let oauth_state = OAuthState {
        state: state_token.clone(),
        provider,
        redirect_after: None,
        pkce_verifier: pkce_verifier.clone(),
        created_at: now,
        expires_at: now + Duration::minutes(10),
        consumed_at: None,
    };
    state.store.create_oauth_state(oauth_state).await?;
    Ok(OAuthLoginResponse {
        provider,
        authorization_url: provider_client
            .authorization_url(&state_token, pkce_verifier.as_deref()),
    })
}

async fn oauth_callback(
    state: AppState,
    provider: AuthProvider,
    query: OAuthCallbackQuery,
) -> Result<Response, ApiFailure> {
    let oauth_state = state
        .store
        .consume_oauth_state(&query.state, provider)
        .await?
        .ok_or_else(|| ApiFailure::bad_request("invalid oauth state"))?;
    if oauth_state.expires_at < Utc::now() {
        return Err(ApiFailure::bad_request("expired oauth state"));
    }
    let provider_client = state
        .providers
        .get(&provider)
        .ok_or_else(|| ApiFailure::not_configured(provider))?;
    let identity = provider_client
        .exchange_code(&query.code, oauth_state.pkce_verifier.as_deref())
        .await?;
    let now = Utc::now();
    let existing = state
        .store
        .find_linked_provider(identity.provider, &identity.provider_user_id)
        .await?;
    let user = if let Some(existing) = &existing {
        state
            .store
            .get_user(existing.user_id)
            .await?
            .ok_or_else(|| ApiFailure::not_found("linked provider user missing"))?
    } else {
        state
            .store
            .upsert_user(User {
                id: Uuid::new_v4(),
                display_name: identity.username.clone(),
                avatar_url: identity.avatar_url.clone(),
                created_at: now,
                updated_at: now,
            })
            .await?
    };

    let linked_provider = LinkedProvider {
        id: existing
            .map(|existing| existing.id)
            .unwrap_or_else(Uuid::new_v4),
        user_id: user.id,
        provider: identity.provider,
        provider_user_id: identity.provider_user_id.clone(),
        provider_username: identity.username.clone(),
        access_token: protect_token(&state.config.session_secret, &identity.access_token),
        refresh_token: identity
            .refresh_token
            .as_ref()
            .map(|token| protect_token(&state.config.session_secret, token)),
        expires_at: identity.expires_at,
        linked_at: now,
    };
    let linked_provider = state.store.upsert_linked_provider(linked_provider).await?;

    if let Ok(sync) = provider_client.sync_list(&identity).await {
        for mut entry in sync.entries {
            entry.user_id = user.id;
            let _ = state.store.upsert_library_entry(entry).await;
        }
    }

    let session_token = random_url_token(48);
    let session = state
        .store
        .create_session(Session {
            id: Uuid::new_v4(),
            user_id: user.id,
            token_hash: hash_token(&state.config.session_secret, &session_token),
            created_at: now,
            expires_at: now + Duration::days(30),
            revoked_at: None,
        })
        .await?;

    let login_result = LoginResult {
        user,
        linked_provider,
        session,
        session_token: session_token.clone(),
    };

    let mut response = Redirect::to(&state.config.web_redirect_url).into_response();
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&session_cookie(&state.config, &session_token))
            .expect("valid cookie"),
    );
    response.headers_mut().append(
        "x-typenx-user-id",
        HeaderValue::from_str(&login_result.user.id.to_string()).expect("uuid header"),
    );
    Ok(response)
}

async fn current_user(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(User, Vec<LinkedProvider>), ApiFailure> {
    let session = session_from_headers(state, headers)
        .await?
        .ok_or_else(|| ApiFailure::unauthorized("missing or invalid session"))?;
    let user = state
        .store
        .get_user(session.user_id)
        .await?
        .ok_or_else(|| ApiFailure::unauthorized("session user missing"))?;
    let providers = state.store.list_linked_providers(user.id).await?;
    Ok((user, providers))
}

async fn session_from_headers(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Option<Session>, ApiFailure> {
    let Some(token) = session_token_from_headers(headers) else {
        return Ok(None);
    };
    let token_hash = hash_token(&state.config.session_secret, &token);
    let Some(session) = state.store.get_session_by_token_hash(&token_hash).await? else {
        return Ok(None);
    };
    if session.revoked_at.is_some() || session.expires_at < Utc::now() {
        return Ok(None);
    }
    Ok(Some(session))
}

fn session_token_from_headers(headers: &HeaderMap) -> Option<String> {
    let cookie = headers.get(header::COOKIE)?.to_str().ok()?;
    cookie.split(';').find_map(|part| {
        let (name, value) = part.trim().split_once('=')?;
        (name == SESSION_COOKIE).then(|| value.to_owned())
    })
}

fn session_cookie(config: &AppConfig, token: &str) -> String {
    let secure = if config.secure_cookies {
        "; Secure"
    } else {
        ""
    };
    format!(
        "{SESSION_COOKIE}={token}; Path=/; Max-Age={}; HttpOnly; SameSite=Lax{secure}",
        30 * 24 * 60 * 60
    )
}

fn expired_session_cookie(config: &AppConfig) -> String {
    let secure = if config.secure_cookies {
        "; Secure"
    } else {
        ""
    };
    format!("{SESSION_COOKIE}=; Path=/; Max-Age=0; HttpOnly; SameSite=Lax{secure}")
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

async fn read_cache<T: serde::de::DeserializeOwned>(
    state: &AppState,
    addon_id: Uuid,
    cache_key: &str,
) -> Result<Option<T>, ApiFailure> {
    let Some(entry) = state.store.get_metadata_cache(addon_id, cache_key).await? else {
        return Ok(None);
    };
    if entry.expires_at <= Utc::now() {
        return Ok(None);
    }
    Ok(Some(serde_json::from_str(&entry.payload_json).map_err(
        |error| ApiFailure {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: error.to_string(),
        },
    )?))
}

async fn write_cache<T: Serialize>(
    state: &AppState,
    addon_id: Uuid,
    cache_key: String,
    payload: &T,
) -> Result<(), ApiFailure> {
    let now = Utc::now();
    state
        .store
        .set_metadata_cache(MetadataCacheEntry {
            id: Uuid::new_v4(),
            addon_id,
            cache_key,
            payload_json: serde_json::to_string(payload).map_err(|error| ApiFailure {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: error.to_string(),
            })?,
            expires_at: now + Duration::hours(1),
            created_at: now,
        })
        .await?;
    Ok(())
}

#[derive(Debug)]
pub struct ApiFailure {
    status: StatusCode,
    message: String,
}

impl ApiFailure {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    fn not_configured(provider: AuthProvider) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: format!("{} oauth is not configured", provider.as_str()),
        }
    }
}

impl IntoResponse for ApiFailure {
    fn into_response(self) -> Response {
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

impl From<typenx_core::providers::ProviderError> for ApiFailure {
    fn from(error: typenx_core::providers::ProviderError) -> Self {
        Self {
            status: StatusCode::BAD_GATEWAY,
            message: error.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;
    use typenx_core::{
        auth::ProviderIdentity,
        library::{ProviderListSync, WatchStatus},
        providers::ProviderError,
    };
    use typenx_storage::memory::MemoryStore;

    #[tokio::test]
    async fn openapi_endpoint_returns_schema() {
        let state = test_state();
        let response = build_router(state)
            .oneshot(Request::get("/openapi.json").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn login_callback_sets_cookie_and_me_reads_session() {
        let state = test_state().with_provider(Arc::new(FakeProvider));
        let router = build_router(state);

        let login_response = router
            .clone()
            .oneshot(
                Request::get("/auth/anilist/login")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(login_response.status(), StatusCode::OK);

        let callback_response = router
            .clone()
            .oneshot(
                Request::get("/auth/anilist/callback?code=test-user&state=state-for-tests")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(callback_response.status(), StatusCode::BAD_REQUEST);

        let login_body = axum::body::to_bytes(login_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let login: OAuthLoginResponse = serde_json::from_slice(&login_body).unwrap();
        let callback_url = login
            .authorization_url
            .split("state=")
            .nth(1)
            .expect("state exists")
            .to_owned();
        let callback_response = router
            .clone()
            .oneshot(
                Request::get(format!(
                    "/auth/anilist/callback?code=test-user&state={callback_url}"
                ))
                .body(Body::empty())
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(callback_response.status(), StatusCode::SEE_OTHER);
        let cookie = callback_response
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();

        let me_response = router
            .oneshot(
                Request::get("/me")
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(me_response.status(), StatusCode::OK);
    }

    fn test_state() -> AppState {
        AppState::from_config(
            Arc::new(MemoryStore::default()),
            AppConfig {
                public_base_url: "http://127.0.0.1:8080".to_owned(),
                web_redirect_url: "http://127.0.0.1:3000".to_owned(),
                session_secret: "test-secret".to_owned(),
                secure_cookies: false,
            },
        )
    }

    struct FakeProvider;

    #[async_trait]
    impl AnimeProviderClient for FakeProvider {
        fn provider(&self) -> AuthProvider {
            AuthProvider::AniList
        }

        fn authorization_url(&self, state: &str, _pkce_challenge: Option<&str>) -> String {
            format!("https://example.test/oauth?state={state}")
        }

        async fn exchange_code(
            &self,
            code: &str,
            _pkce_verifier: Option<&str>,
        ) -> Result<ProviderIdentity, ProviderError> {
            Ok(ProviderIdentity {
                provider: AuthProvider::AniList,
                provider_user_id: "100".to_owned(),
                username: code.to_owned(),
                avatar_url: None,
                access_token: "access-token".to_owned(),
                refresh_token: None,
                expires_at: None,
            })
        }

        async fn sync_list(
            &self,
            identity: &ProviderIdentity,
        ) -> Result<ProviderListSync, ProviderError> {
            Ok(ProviderListSync {
                provider: identity.provider,
                entries: vec![typenx_core::library::AnimeListEntry {
                    id: Uuid::new_v4(),
                    user_id: Uuid::nil(),
                    provider: identity.provider,
                    provider_anime_id: "1".to_owned(),
                    title: "Frieren".to_owned(),
                    status: WatchStatus::Watching,
                    score: Some(10.0),
                    progress_episodes: 4,
                    total_episodes: Some(28),
                    updated_at: Utc::now(),
                }],
                synced_at: Utc::now(),
            })
        }
    }
}
