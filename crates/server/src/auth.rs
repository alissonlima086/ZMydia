use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    Json,
    extract::{Path, Request, State},
    http::{HeaderMap, Method, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use base64::Engine;
use ring::{
    digest::{Context, SHA256},
    rand::SecureRandom,
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::{config::SecurityConfig, crypto, web::AppState};

pub const SESSION_COOKIE_NAME: &str = "mymedia_session";
const SESSION_TTL_DAYS: i64 = 30;
const MAX_FAILED_ATTEMPTS: u32 = 5;
const LOCKOUT_DURATION: Duration = Duration::from_secs(15 * 60);
const RATE_ENTRY_TTL: Duration = Duration::from_secs(30 * 60);
const MAX_RATE_ENTRIES: usize = 2_048;
const MAX_API_TOKENS: i64 = 8;
const API_TOKEN_TTL_DAYS: i64 = 90;

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthStatusResponse {
    pub authenticated: bool,
    pub username: Option<String>,
    pub setup_required: bool,
    pub must_change_password: bool,
}

#[derive(Debug, Deserialize)]
pub struct SetupRequest {
    pub username: String,
    pub password: String,
}
#[derive(Debug, Deserialize)]
pub struct RecoverPasswordRequest {
    pub username: String,
    pub recovery_code: String,
    pub new_password: String,
}
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub remember_me: Option<bool>,
}
#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub success: bool,
    pub username: String,
    pub must_change_password: bool,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct UserProfileResponse {
    pub id: i64,
    pub username: String,
    pub display_name: Option<String>,
    pub created_at: String,
}
#[derive(Debug, Deserialize)]
pub struct UpdateProfileRequest {
    pub username: String,
    pub display_name: Option<String>,
}
#[derive(Debug, Deserialize)]
pub struct UpdatePasswordRequest {
    pub current_password: String,
    pub new_password: String,
    #[serde(default)]
    pub revoke_api_tokens: bool,
}
#[derive(Debug, Deserialize)]
pub struct CreateApiTokenRequest {
    pub name: Option<String>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct CreateApiTokenResponse {
    pub id: i64,
    pub name: String,
    pub token: String,
    pub token_hint: String,
    pub scopes: String,
    pub created_at: String,
    pub expires_at: String,
}
#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct ApiTokenItem {
    pub id: i64,
    pub name: String,
    pub token_hint: String,
    pub scopes: String,
    pub last_used_at: Option<String>,
    pub created_at: String,
    pub expires_at: Option<String>,
}
#[derive(Debug, Deserialize)]
pub struct PairingStartRequest {
    pub name: Option<String>,
    pub extension_origin: Option<String>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct PairingChallengeResponse {
    pub id: String,
    pub display_code: String,
    pub expires_in_seconds: u32,
}
#[derive(Debug, Deserialize)]
pub struct PairingConfirmRequest {
    pub code: String,
}

#[derive(Clone, Debug)]
pub struct AuthenticatedUser {
    pub id: i64,
    pub username: String,
    pub scopes: Option<String>,
    pub session_hash: Option<String>,
    pub extension_origin: Option<String>,
    pub principal_key: String,
}

impl AuthenticatedUser {
    pub fn is_web_session(&self) -> bool {
        self.scopes.is_none()
    }
    pub fn has_scope(&self, scope: &str) -> bool {
        self.scopes
            .as_ref()
            .is_none_or(|values| values.split(',').any(|value| value.trim() == scope))
    }
}

#[derive(Clone, Default)]
pub struct RateLimiter {
    attempts: Arc<Mutex<HashMap<String, (u32, Instant)>>>,
}
impl RateLimiter {
    fn prune(map: &mut HashMap<String, (u32, Instant)>) {
        map.retain(|_, (_, at)| at.elapsed() < RATE_ENTRY_TTL);
        if map.len() >= MAX_RATE_ENTRIES {
            let mut oldest: Vec<_> = map
                .iter()
                .map(|(key, (_, at))| (key.clone(), *at))
                .collect();
            oldest.sort_by_key(|(_, at)| *at);
            for (key, _) in oldest.into_iter().take(map.len() - MAX_RATE_ENTRIES + 1) {
                map.remove(&key);
            }
        }
    }
    pub fn is_locked(&self, key: &str) -> bool {
        let mut map = self.attempts.lock().expect("rate limiter lock");
        Self::prune(&mut map);
        map.get(key).is_some_and(|(count, at)| {
            *count >= MAX_FAILED_ATTEMPTS && at.elapsed() < LOCKOUT_DURATION
        })
    }
    pub fn record_failure(&self, key: &str) {
        let mut map = self.attempts.lock().expect("rate limiter lock");
        Self::prune(&mut map);
        let entry = map.entry(key.to_owned()).or_insert((0, Instant::now()));
        entry.0 += 1;
        entry.1 = Instant::now();
    }
    pub fn record_success(&self, key: &str) {
        self.attempts.lock().expect("rate limiter lock").remove(key);
    }
    #[cfg(test)]
    fn len(&self) -> usize {
        self.attempts.lock().unwrap().len()
    }
}

fn hash_secret(value: &str) -> String {
    let mut ctx = Context::new(&SHA256);
    ctx.update(value.as_bytes());
    ctx.finish()
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn random_token(prefix: &str, bytes: usize) -> Result<String, ()> {
    let mut raw = vec![0_u8; bytes];
    ring::rand::SystemRandom::new()
        .fill(&mut raw)
        .map_err(|_| ())?;
    Ok(format!(
        "{prefix}{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw)
    ))
}
fn pairing_code() -> Result<String, ()> {
    let mut raw = [0_u8; 4];
    ring::rand::SystemRandom::new()
        .fill(&mut raw)
        .map_err(|_| ())?;
    Ok(format!("{:06}", u32::from_be_bytes(raw) % 1_000_000))
}
fn json_error(status: StatusCode, message: &'static str) -> Response {
    let code = match status {
        StatusCode::UNAUTHORIZED => "unauthorized",
        StatusCode::FORBIDDEN => "forbidden",
        StatusCode::NOT_FOUND => "not_found",
        StatusCode::CONFLICT => "conflict",
        StatusCode::TOO_MANY_REQUESTS => "rate_limited",
        _ if status.is_client_error() => "invalid_request",
        _ => "internal_error",
    };
    (status, Json(crate::error::error_json(code, message))).into_response()
}
fn request_key(headers: &HeaderMap, security: &SecurityConfig, username: &str) -> String {
    let address = security
        .trust_forwarded_for
        .then(|| {
            headers
                .get("x-forwarded-for")?
                .to_str()
                .ok()?
                .split(',')
                .next()
                .map(str::trim)
        })
        .flatten()
        .unwrap_or("direct");
    format!("{address}:{}", username.to_ascii_lowercase())
}
fn cookie(token: &str, remember: bool, security: &SecurityConfig) -> String {
    let mut value = format!(
        "{SESSION_COOKIE_NAME}={token}; HttpOnly; SameSite={}; Path=/",
        security.cookie_same_site
    );
    if remember {
        value.push_str(&format!("; Max-Age={}", SESSION_TTL_DAYS * 86400));
    }
    if security.cookie_secure {
        value.push_str("; Secure");
    }
    if let Some(domain) = &security.cookie_domain {
        value.push_str(&format!("; Domain={domain}"));
    }
    value
}
fn clear_cookie(security: &SecurityConfig) -> String {
    format!("{}; Max-Age=0", cookie("", false, security))
}

pub fn extract_session_cookie(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|part| {
            part.trim()
                .strip_prefix(&format!("{SESSION_COOKIE_NAME}="))
                .map(str::to_owned)
        })
}
pub fn extract_bearer_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .trim()
        .strip_prefix("Bearer ")
        .map(|value| value.trim().to_owned())
}

pub async fn validate_session_or_token(
    pool: &SqlitePool,
    session: Option<&str>,
    bearer: Option<&str>,
) -> Option<AuthenticatedUser> {
    if let Some(raw) = session {
        let session_hash = hash_secret(raw);
        let row = sqlx::query_as::<_, (i64, String)>(
            "SELECT u.id, u.username FROM sessions s JOIN users u ON u.id=s.user_id WHERE s.id=? AND s.expires_at > datetime('now')")
            .bind(&session_hash).fetch_optional(pool).await.ok().flatten();
        if let Some((id, username)) = row {
            let _ = sqlx::query("UPDATE sessions SET last_active_at=datetime('now') WHERE id=? AND last_active_at < datetime('now','-5 minutes')").bind(&session_hash).execute(pool).await;
            return Some(AuthenticatedUser {
                id,
                username,
                scopes: None,
                session_hash: Some(session_hash.clone()),
                extension_origin: None,
                principal_key: format!("session:{session_hash}"),
            });
        }
    }
    if let Some(raw) = bearer {
        let token_hash = hash_secret(raw);
        let row = sqlx::query_as::<_, (i64, String, String, Option<String>)>(
            "SELECT u.id,u.username,t.scopes,t.extension_origin FROM api_tokens t JOIN users u ON u.id=t.user_id WHERE t.token_hash=? AND t.revoked_at IS NULL AND (t.expires_at IS NULL OR t.expires_at > datetime('now'))")
            .bind(&token_hash).fetch_optional(pool).await.ok().flatten();
        if let Some((id, username, scopes, extension_origin)) = row {
            let _ = sqlx::query("UPDATE api_tokens SET last_used_at=datetime('now') WHERE token_hash=? AND (last_used_at IS NULL OR last_used_at < datetime('now','-5 minutes'))").bind(&token_hash).execute(pool).await;
            return Some(AuthenticatedUser {
                id,
                username,
                scopes: Some(scopes),
                session_hash: None,
                extension_origin,
                principal_key: format!("token:{token_hash}"),
            });
        }
    }
    None
}

pub(crate) async fn get_auth_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Json<AuthStatusResponse> {
    let setup_required = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0)
        == 0;
    let user = validate_session_or_token(
        &state.pool,
        extract_session_cookie(&headers).as_deref(),
        extract_bearer_token(&headers).as_deref(),
    )
    .await;
    Json(AuthStatusResponse {
        authenticated: user.is_some(),
        username: user.as_ref().map(|u| u.username.clone()),
        setup_required,
        // Mantido temporariamente no contrato HTTP para clientes antigos.
        must_change_password: false,
    })
}

pub(crate) async fn setup_endpoint(
    State(state): State<AppState>,
    Json(req): Json<SetupRequest>,
) -> Response {
    let username = req.username.trim();
    if !valid_username(username) {
        return json_error(StatusCode::BAD_REQUEST, "Nome de usuário inválido");
    }
    if let Err(message) = validate_password(&req.password) {
        return json_error(StatusCode::BAD_REQUEST, message);
    }
    let hash = match crypto::hash_password(&req.password) {
        Ok(hash) => hash,
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Não foi possível concluir o setup",
            );
        }
    };
    let recovery_code = match random_token("mm_recovery_", 24) {
        Ok(code) => code,
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Não foi possível gerar o código de recuperação",
            );
        }
    };
    let result = sqlx::query("INSERT INTO users(username,password_hash,display_name,recovery_code_hash) SELECT ?,?,?,? WHERE NOT EXISTS(SELECT 1 FROM users)")
        .bind(username).bind(hash).bind(username).bind(hash_secret(&recovery_code)).execute(&state.pool).await;
    let Ok(result) = result else {
        return json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Não foi possível concluir o setup",
        );
    };
    if result.rows_affected() != 1 {
        return json_error(StatusCode::CONFLICT, "O setup inicial já foi concluído");
    }
    (
        StatusCode::CREATED,
        Json(serde_json::json!({"success":true,"recovery_code":recovery_code})),
    )
        .into_response()
}

pub(crate) async fn recover_password_endpoint(
    State(state): State<AppState>,
    Json(req): Json<RecoverPasswordRequest>,
) -> Response {
    if let Err(message) = validate_password(&req.new_password) {
        return json_error(StatusCode::BAD_REQUEST, message);
    }
    let row = sqlx::query_as::<_, (i64, Option<String>)>(
        "SELECT id,recovery_code_hash FROM users WHERE username=? COLLATE NOCASE",
    )
    .bind(req.username.trim())
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten();
    let Some((user_id, Some(expected_hash))) = row else {
        return json_error(StatusCode::UNAUTHORIZED, "Código de recuperação inválido");
    };
    if hash_secret(req.recovery_code.trim()) != expected_hash {
        return json_error(StatusCode::UNAUTHORIZED, "Código de recuperação inválido");
    }
    let password_hash = match crypto::hash_password(&req.new_password) {
        Ok(hash) => hash,
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Não foi possível redefinir a senha",
            );
        }
    };
    let replacement_code = match random_token("mm_recovery_", 24) {
        Ok(code) => code,
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Não foi possível renovar o código de recuperação",
            );
        }
    };
    let mut transaction = match state.pool.begin().await {
        Ok(transaction) => transaction,
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Não foi possível redefinir a senha",
            );
        }
    };
    let update = sqlx::query("UPDATE users SET password_hash=?,recovery_code_hash=?,must_change_password=0,password_changed_at=datetime('now'),updated_at=datetime('now') WHERE id=?")
        .bind(password_hash).bind(hash_secret(&replacement_code)).bind(user_id).execute(&mut *transaction).await;
    if update.is_err()
        || sqlx::query("DELETE FROM sessions WHERE user_id=?").bind(user_id).execute(&mut *transaction).await.is_err()
        || sqlx::query("UPDATE api_tokens SET revoked_at=datetime('now') WHERE user_id=? AND revoked_at IS NULL").bind(user_id).execute(&mut *transaction).await.is_err()
        || transaction.commit().await.is_err()
    {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, "Não foi possível redefinir a senha");
    }
    (StatusCode::OK, Json(serde_json::json!({"success":true,"recovery_code":replacement_code,"sessions_revoked":true,"api_tokens_revoked":true}))).into_response()
}

pub(crate) async fn login_endpoint(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<LoginRequest>,
) -> Response {
    let username = req.username.trim();
    let key = request_key(&headers, &state.security, username);
    if state.rate_limiter.is_locked(&key) {
        return json_error(
            StatusCode::TOO_MANY_REQUESTS,
            "Muitas tentativas falhas; tente novamente mais tarde",
        );
    }
    let row = sqlx::query_as::<_, (i64, String, String)>(
        "SELECT id,username,password_hash FROM users WHERE username=? COLLATE NOCASE",
    )
    .bind(username)
    .fetch_optional(&state.pool)
    .await
    .ok()
    .flatten();
    let Some((user_id, canonical, password_hash)) = row else {
        state.rate_limiter.record_failure(&key);
        tokio::time::sleep(Duration::from_millis(200)).await;
        return json_error(StatusCode::UNAUTHORIZED, "Usuário ou senha inválidos");
    };
    if !crypto::verify_password(&req.password, &password_hash) {
        state.rate_limiter.record_failure(&key);
        tokio::time::sleep(Duration::from_millis(200)).await;
        return json_error(StatusCode::UNAUTHORIZED, "Usuário ou senha inválidos");
    }
    state.rate_limiter.record_success(&key);
    let raw = match random_token("mm_session_", 32) {
        Ok(raw) => raw,
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Não foi possível criar a sessão",
            );
        }
    };
    let remember = req.remember_me.unwrap_or(false);
    let ttl = if remember { SESSION_TTL_DAYS } else { 1 };
    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.chars().take(255).collect::<String>());
    if sqlx::query("INSERT INTO sessions(id,user_id,user_agent,ip_address,expires_at) VALUES(?,?,?,?,datetime('now','+'||?||' days'))")
        .bind(hash_secret(&raw)).bind(user_id).bind(user_agent).bind(&key).bind(ttl).execute(&state.pool).await.is_err() { return json_error(StatusCode::INTERNAL_SERVER_ERROR,"Não foi possível criar a sessão"); }
    let mut response = Json(LoginResponse {
        success: true,
        username: canonical,
        // Campo legado mantido apenas para compatibilidade de clientes.
        must_change_password: false,
    })
    .into_response();
    if let Ok(value) = cookie(&raw, remember, &state.security).parse() {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
    response
}

pub(crate) async fn logout_endpoint(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(raw) = extract_session_cookie(&headers) {
        let _ = sqlx::query("DELETE FROM sessions WHERE id=?")
            .bind(hash_secret(&raw))
            .execute(&state.pool)
            .await;
    }
    let mut response = Json(serde_json::json!({"success":true})).into_response();
    if let Ok(value) = clear_cookie(&state.security).parse() {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
    response
}

pub(crate) async fn csrf_endpoint(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let Some(raw_session) = extract_session_cookie(&headers) else {
        return json_error(StatusCode::UNAUTHORIZED, "Autenticação necessária");
    };
    let Some(user) = validate_session_or_token(&state.pool, Some(&raw_session), None).await else {
        return json_error(StatusCode::UNAUTHORIZED, "Autenticação necessária");
    };
    let token = match random_token("mm_csrf_", 24) {
        Ok(token) => token,
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Não foi possível gerar proteção CSRF",
            );
        }
    };
    if sqlx::query("UPDATE sessions SET csrf_hash=? WHERE id=?")
        .bind(hash_secret(&token))
        .bind(user.session_hash)
        .execute(&state.pool)
        .await
        .is_err()
    {
        return json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Não foi possível gerar proteção CSRF",
        );
    }
    Json(serde_json::json!({"token":token})).into_response()
}

pub(crate) async fn current_authenticated_user(
    headers: &HeaderMap,
    pool: &SqlitePool,
) -> Option<AuthenticatedUser> {
    validate_session_or_token(
        pool,
        extract_session_cookie(headers).as_deref(),
        extract_bearer_token(headers).as_deref(),
    )
    .await
}

pub(crate) async fn get_user_profile(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<UserProfileResponse>, (StatusCode, String)> {
    let user = current_authenticated_user(&headers, &state.pool)
        .await
        .ok_or((StatusCode::UNAUTHORIZED, "Não autenticado".into()))?;
    let row = sqlx::query_as::<_, (i64, String, Option<String>, String)>(
        "SELECT id,username,display_name,created_at FROM users WHERE id=?",
    )
    .bind(user.id)
    .fetch_one(&state.pool)
    .await
    .map_err(internal)?;
    Ok(Json(UserProfileResponse {
        id: row.0,
        username: row.1,
        display_name: row.2,
        created_at: row.3,
    }))
}
pub(crate) async fn update_user_profile(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<UpdateProfileRequest>,
) -> Result<Json<UserProfileResponse>, (StatusCode, String)> {
    let user = current_authenticated_user(&headers, &state.pool)
        .await
        .ok_or((StatusCode::UNAUTHORIZED, "Não autenticado".into()))?;
    let username = req.username.trim();
    if !valid_username(username) {
        return Err((StatusCode::BAD_REQUEST, "Nome de usuário inválido".into()));
    }
    let display = req
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or(username);
    sqlx::query("UPDATE users SET username=?,display_name=?,updated_at=datetime('now') WHERE id=?")
        .bind(username)
        .bind(display)
        .bind(user.id)
        .execute(&state.pool)
        .await
        .map_err(|_| (StatusCode::CONFLICT, "Nome de usuário indisponível".into()))?;
    get_user_profile(State(state), headers).await
}
pub(crate) async fn update_user_password(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<UpdatePasswordRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let user = current_authenticated_user(&headers, &state.pool)
        .await
        .ok_or((StatusCode::UNAUTHORIZED, "Não autenticado".into()))?;
    let current = sqlx::query_scalar::<_, String>("SELECT password_hash FROM users WHERE id=?")
        .bind(user.id)
        .fetch_one(&state.pool)
        .await
        .map_err(internal)?;
    if !crypto::verify_password(&req.current_password, &current) {
        return Err((StatusCode::UNAUTHORIZED, "Senha atual incorreta".into()));
    }
    validate_password(&req.new_password)
        .map_err(|message| (StatusCode::BAD_REQUEST, message.into()))?;
    let hash = crypto::hash_password(&req.new_password).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Não foi possível atualizar a senha".into(),
        )
    })?;
    let mut tx = state.pool.begin().await.map_err(internal)?;
    sqlx::query("UPDATE users SET password_hash=?,must_change_password=0,password_changed_at=datetime('now'),updated_at=datetime('now') WHERE id=?").bind(hash).bind(user.id).execute(&mut *tx).await.map_err(internal)?;
    if let Some(current_session) = user.session_hash {
        sqlx::query("DELETE FROM sessions WHERE user_id=? AND id<>?")
            .bind(user.id)
            .bind(current_session)
            .execute(&mut *tx)
            .await
            .map_err(internal)?;
    }
    if req.revoke_api_tokens {
        sqlx::query("UPDATE api_tokens SET revoked_at=datetime('now') WHERE user_id=? AND revoked_at IS NULL").bind(user.id).execute(&mut *tx).await.map_err(internal)?;
    }
    tx.commit().await.map_err(internal)?;
    Ok(Json(
        serde_json::json!({"success":true,"other_sessions_revoked":true,"api_tokens_revoked":req.revoke_api_tokens}),
    ))
}

fn valid_username(value: &str) -> bool {
    (3..=50).contains(&value.len())
        && value
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
}
fn validate_password(password: &str) -> Result<(), &'static str> {
    if password.chars().count() < 8 {
        return Err("A senha deve ter pelo menos 8 caracteres");
    }
    Ok(())
}
fn internal(error: impl std::fmt::Display) -> (StatusCode, String) {
    tracing::error!(%error, "internal authentication failure");
    (StatusCode::INTERNAL_SERVER_ERROR, "Erro interno".into())
}

async fn issue_api_token(
    pool: &SqlitePool,
    user_id: i64,
    name: &str,
    origin: Option<&str>,
) -> Result<CreateApiTokenResponse, (StatusCode, String)> {
    let count=sqlx::query_scalar::<_,i64>("SELECT COUNT(*) FROM api_tokens WHERE user_id=? AND revoked_at IS NULL AND (expires_at IS NULL OR expires_at>datetime('now'))").bind(user_id).fetch_one(pool).await.map_err(internal)?;
    if count >= MAX_API_TOKENS {
        return Err((
            StatusCode::CONFLICT,
            "Limite de credenciais ativas atingido".into(),
        ));
    }
    let token = random_token("mm_ext_", 32).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Não foi possível gerar a credencial".into(),
        )
    })?;
    let hint = format!("••••{}", &token[token.len() - 4..]);
    let scopes = "downloads:read,downloads:write";
    let result=sqlx::query("INSERT INTO api_tokens(user_id,token_hash,token_hint,name,scopes,expires_at,extension_origin) VALUES(?,?,?,?,?,datetime('now','+'||?||' days'),?)")
        .bind(user_id).bind(hash_secret(&token)).bind(&hint).bind(name).bind(scopes).bind(API_TOKEN_TTL_DAYS).bind(origin).execute(pool).await.map_err(internal)?;
    let (created, expires) = sqlx::query_as::<_, (String, String)>(
        "SELECT created_at,expires_at FROM api_tokens WHERE id=?",
    )
    .bind(result.last_insert_rowid())
    .fetch_one(pool)
    .await
    .map_err(internal)?;
    Ok(CreateApiTokenResponse {
        id: result.last_insert_rowid(),
        name: name.into(),
        token,
        token_hint: hint,
        scopes: scopes.into(),
        created_at: created,
        expires_at: expires,
    })
}
pub(crate) async fn create_pairing_challenge(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<PairingStartRequest>,
) -> Result<Json<PairingChallengeResponse>, (StatusCode, String)> {
    let user = current_authenticated_user(&headers, &state.pool)
        .await
        .ok_or((StatusCode::UNAUTHORIZED, "Não autenticado".into()))?;
    if !user.is_web_session() {
        return Err((StatusCode::FORBIDDEN, "Sessão web necessária".into()));
    }
    let name = req
        .name
        .unwrap_or_else(|| "Extensão do navegador".into())
        .trim()
        .chars()
        .take(64)
        .collect::<String>();
    if name.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Nome inválido".into()));
    }
    let origin = req
        .extension_origin
        .filter(|origin| crate::config::is_extension_origin(origin))
        .ok_or((
            StatusCode::BAD_REQUEST,
            "A extensão não respondeu ao desafio de pareamento".into(),
        ))?;
    let id = random_token("pair_", 18).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Falha ao iniciar pareamento".into(),
        )
    })?;
    let code = pairing_code().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Falha ao iniciar pareamento".into(),
        )
    })?;
    sqlx::query("INSERT INTO extension_pairing_challenges(id,user_id,code_hash,display_code,token_name,extension_origin,expires_at) VALUES(?,?,?,?,?,?,datetime('now','+5 minutes'))").bind(&id).bind(user.id).bind(hash_secret(&code)).bind("******").bind(name).bind(origin).execute(&state.pool).await.map_err(internal)?;
    Ok(Json(PairingChallengeResponse {
        id,
        display_code: code,
        expires_in_seconds: 300,
    }))
}
pub(crate) async fn confirm_pairing_challenge(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<PairingConfirmRequest>,
) -> Result<Json<CreateApiTokenResponse>, (StatusCode, String)> {
    let user = current_authenticated_user(&headers, &state.pool)
        .await
        .ok_or((StatusCode::UNAUTHORIZED, "Não autenticado".into()))?;
    if !user.is_web_session() {
        return Err((StatusCode::FORBIDDEN, "Sessão web necessária".into()));
    }
    let row=sqlx::query_as::<_,(String,String,Option<String>)>("SELECT code_hash,token_name,extension_origin FROM extension_pairing_challenges WHERE id=? AND user_id=? AND confirmed_at IS NULL AND expires_at>datetime('now')").bind(&id).bind(user.id).fetch_optional(&state.pool).await.map_err(internal)?.ok_or((StatusCode::NOT_FOUND,"Desafio expirado ou inexistente".into()))?;
    if hash_secret(req.code.trim().to_ascii_uppercase().as_str()) != row.0 {
        return Err((
            StatusCode::UNAUTHORIZED,
            "Código de pareamento inválido".into(),
        ));
    }
    let token = issue_api_token(&state.pool, user.id, &row.1, row.2.as_deref()).await?;
    sqlx::query("UPDATE extension_pairing_challenges SET confirmed_at=datetime('now') WHERE id=?")
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(internal)?;
    Ok(Json(token))
}
pub(crate) async fn create_api_token(
    State(_): State<AppState>,
    _: HeaderMap,
    Json(_): Json<CreateApiTokenRequest>,
) -> Result<Json<CreateApiTokenResponse>, (StatusCode, String)> {
    Err((
        StatusCode::GONE,
        "Use o fluxo de pareamento com confirmação".into(),
    ))
}
pub(crate) async fn list_api_tokens(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<ApiTokenItem>>, (StatusCode, String)> {
    let user = current_authenticated_user(&headers, &state.pool)
        .await
        .ok_or((StatusCode::UNAUTHORIZED, "Não autenticado".into()))?;
    if !user.is_web_session() {
        return Err((StatusCode::FORBIDDEN, "Sessão web necessária".into()));
    }
    let items=sqlx::query_as::<_,ApiTokenItem>("SELECT id,name,token_hint,scopes,last_used_at,created_at,expires_at FROM api_tokens WHERE user_id=? AND revoked_at IS NULL AND (expires_at IS NULL OR expires_at>datetime('now')) ORDER BY id DESC").bind(user.id).fetch_all(&state.pool).await.map_err(internal)?;
    Ok(Json(items))
}
pub(crate) async fn revoke_api_token(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<StatusCode, (StatusCode, String)> {
    let user = current_authenticated_user(&headers, &state.pool)
        .await
        .ok_or((StatusCode::UNAUTHORIZED, "Não autenticado".into()))?;
    if !user.is_web_session() {
        return Err((StatusCode::FORBIDDEN, "Sessão web necessária".into()));
    }
    sqlx::query("UPDATE api_tokens SET revoked_at=datetime('now') WHERE id=? AND user_id=?")
        .bind(id)
        .bind(user.id)
        .execute(&state.pool)
        .await
        .map_err(internal)?;
    Ok(StatusCode::NO_CONTENT)
}

pub fn required_scope(method: &Method, path: &str) -> Option<&'static str> {
    if path.starts_with("/api/health") && *method == Method::GET {
        return Some("server:health");
    }
    if path == "/api/downloads" && *method == Method::GET
        || path.starts_with("/api/downloads/") && *method == Method::GET
    {
        return Some("downloads:read");
    }
    if (path == "/api/downloads" || path == "/api/downloads/probe") && *method == Method::POST
        || path.starts_with("/api/downloads/") && *method == Method::DELETE
    {
        return Some("downloads:write");
    }
    None
}
pub(crate) async fn auth_middleware(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path();
    if is_public_route(path) {
        return next.run(request).await;
    }
    let user = validate_session_or_token(
        &state.pool,
        extract_session_cookie(request.headers()).as_deref(),
        extract_bearer_token(request.headers()).as_deref(),
    )
    .await;
    let Some(user) = user else {
        return json_error(StatusCode::UNAUTHORIZED, "Autenticação necessária");
    };
    if !user.is_web_session() {
        let Some(scope) = required_scope(request.method(), path) else {
            return json_error(StatusCode::FORBIDDEN, "Token sem permissão para esta rota");
        };
        if !user.has_scope(scope) {
            return json_error(StatusCode::FORBIDDEN, "Escopo insuficiente");
        }
        if let Some(origin) = request
            .headers()
            .get(header::ORIGIN)
            .and_then(|value| value.to_str().ok())
            .filter(|value| crate::config::is_extension_origin(value))
            && user.extension_origin.as_deref() != Some(origin)
        {
            return json_error(StatusCode::FORBIDDEN, "Origem da extensão não autorizada");
        }
    }
    request.extensions_mut().insert(user);
    next.run(request).await
}
pub fn is_public_route(path: &str) -> bool {
    matches!(
        path,
        "/api/auth/status"
            | "/api/auth/setup"
            | "/api/auth/recover"
            | "/api/auth/login"
            | "/api/health"
            | "/api/health/live"
            | "/api/health/ready"
            | "/api/extension/info"
            | "/extension/zmydia.xpi"
            | "/extension/mymedia.xpi"
            | "/extension/zmydia"
            | "/extension/zmydia.zip"
    ) || !path.starts_with("/api/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;
    #[test]
    fn password_policy_requires_only_eight_characters() {
        assert!(validate_password("1234567").is_err());
        assert!(validate_password("12345678").is_ok());
        assert!(validate_password("password").is_ok());
        assert!(validate_password("áéíóú123").is_ok());
    }
    #[test]
    fn authorization_matrix_is_method_and_route_specific() {
        let allowed = [
            (Method::GET, "/api/downloads", Some("downloads:read")),
            (Method::GET, "/api/downloads/1/file", Some("downloads:read")),
            (Method::POST, "/api/downloads", Some("downloads:write")),
            (
                Method::POST,
                "/api/downloads/probe",
                Some("downloads:write"),
            ),
            (Method::DELETE, "/api/downloads/1", Some("downloads:write")),
        ];
        for (method, path, scope) in allowed {
            assert_eq!(required_scope(&method, path), scope, "{method} {path}");
        }
        for (method, path) in [
            (Method::GET, "/api/media"),
            (Method::POST, "/api/media"),
            (Method::PUT, "/api/settings/downloads"),
            (Method::DELETE, "/api/sources/1"),
        ] {
            assert_eq!(required_scope(&method, path), None, "{method} {path}");
        }
    }
    #[test]
    fn rate_limiter_is_bounded() {
        let limiter = RateLimiter::default();
        for i in 0..(MAX_RATE_ENTRIES + 100) {
            limiter.record_failure(&i.to_string());
        }
        assert!(limiter.len() <= MAX_RATE_ENTRIES);
    }
    #[tokio::test]
    async fn fresh_install_has_no_known_credentials_and_session_is_hashed() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users")
                .fetch_one(&pool)
                .await
                .unwrap(),
            0
        );
        let state = crate::web::AppState::for_test(pool.clone());
        let setup = setup_endpoint(
            State(state.clone()),
            Json(SetupRequest {
                username: "owner".into(),
                password: "Bambu-Verde-47!Lua".into(),
            }),
        )
        .await;
        assert_eq!(setup.status(), StatusCode::CREATED);
        let login = login_endpoint(
            State(state.clone()),
            HeaderMap::new(),
            Json(LoginRequest {
                username: "owner".into(),
                password: "Bambu-Verde-47!Lua".into(),
                remember_me: Some(true),
            }),
        )
        .await;
        assert_eq!(login.status(), StatusCode::OK);
        let set_cookie = login
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap();
        let raw = set_cookie
            .split(';')
            .next()
            .unwrap()
            .split_once('=')
            .unwrap()
            .1;
        let stored = sqlx::query_scalar::<_, String>("SELECT id FROM sessions")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_ne!(stored, raw);
        assert_eq!(stored, hash_secret(raw));
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_str(&format!("{SESSION_COOKIE_NAME}={raw}")).unwrap(),
        );
        assert!(current_authenticated_user(&headers, &pool).await.is_some());
        let csrf = csrf_endpoint(State(state), headers).await;
        assert_eq!(csrf.status(), StatusCode::OK);
        assert!(
            sqlx::query_scalar::<_, Option<String>>("SELECT csrf_hash FROM sessions")
                .fetch_one(&pool)
                .await
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn recovery_code_resets_password_and_revokes_access() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        let old_password = crypto::hash_password("senha-antiga").unwrap();
        let user_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO users(username,password_hash,recovery_code_hash) VALUES('owner',?,?) RETURNING id",
        )
        .bind(old_password)
        .bind(hash_secret("codigo-offline"))
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO sessions(id,user_id,expires_at) VALUES('session-hash',?,datetime('now','+1 day'))")
            .bind(user_id).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO api_tokens(user_id,token_hash,token_hint,name,scopes) VALUES(?,'token-hash','hint','test','downloads:write')")
            .bind(user_id).execute(&pool).await.unwrap();

        let response = recover_password_endpoint(
            State(crate::web::AppState::for_test(pool.clone())),
            Json(RecoverPasswordRequest {
                username: "owner".into(),
                recovery_code: "codigo-offline".into(),
                new_password: "senha-nova-123".into(),
            }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let password_hash =
            sqlx::query_scalar::<_, String>("SELECT password_hash FROM users WHERE id=?")
                .bind(user_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(crypto::verify_password("senha-nova-123", &password_hash));
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sessions WHERE user_id=?")
                .bind(user_id)
                .fetch_one(&pool)
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM api_tokens WHERE user_id=? AND revoked_at IS NULL"
            )
            .bind(user_id)
            .fetch_one(&pool)
            .await
            .unwrap(),
            0
        );
        assert_ne!(
            sqlx::query_scalar::<_, String>("SELECT recovery_code_hash FROM users WHERE id=?")
                .bind(user_id)
                .fetch_one(&pool)
                .await
                .unwrap(),
            hash_secret("codigo-offline")
        );
    }
    #[tokio::test]
    async fn expired_extension_token_is_rejected() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        let hash = crypto::hash_password("Bambu-Verde-47!Lua").unwrap();
        sqlx::query("INSERT INTO users(username,password_hash) VALUES('owner',?)")
            .bind(hash)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO api_tokens(user_id,token_hash,token_hint,name,scopes,expires_at) VALUES(1,?,'x','expired','downloads:read',datetime('now','-1 minute'))").bind(hash_secret("expired-token")).execute(&pool).await.unwrap();
        assert!(
            validate_session_or_token(&pool, None, Some("expired-token"))
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn password_change_revokes_other_sessions_but_keeps_current_session() {
        let pool = crate::db::connect("sqlite::memory:").await.unwrap();
        let hash = crypto::hash_password("Bambu-Verde-47!Lua").unwrap();
        sqlx::query("INSERT INTO users(username,password_hash) VALUES('owner',?)")
            .bind(hash)
            .execute(&pool)
            .await
            .unwrap();
        let state = crate::web::AppState::for_test(pool.clone());
        let login = |state: AppState| async move {
            login_endpoint(
                State(state),
                HeaderMap::new(),
                Json(LoginRequest {
                    username: "owner".into(),
                    password: "Bambu-Verde-47!Lua".into(),
                    remember_me: Some(true),
                }),
            )
            .await
        };
        let first = login(state.clone()).await;
        let second = login(state.clone()).await;
        let raw = |response: &Response| {
            response
                .headers()
                .get(header::SET_COOKIE)
                .unwrap()
                .to_str()
                .unwrap()
                .split(';')
                .next()
                .unwrap()
                .split_once('=')
                .unwrap()
                .1
                .to_owned()
        };
        let first_raw = raw(&first);
        let second_raw = raw(&second);
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_str(&format!("{SESSION_COOKIE_NAME}={first_raw}")).unwrap(),
        );
        let _ = update_user_password(
            State(state),
            headers,
            Json(UpdatePasswordRequest {
                current_password: "Bambu-Verde-47!Lua".into(),
                new_password: "Cacto-Azul-83!Sol".into(),
                revoke_api_tokens: false,
            }),
        )
        .await
        .unwrap();
        assert!(
            validate_session_or_token(&pool, Some(&first_raw), None)
                .await
                .is_some()
        );
        assert!(
            validate_session_or_token(&pool, Some(&second_raw), None)
                .await
                .is_none()
        );
    }
}
