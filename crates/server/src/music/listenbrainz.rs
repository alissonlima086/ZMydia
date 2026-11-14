use crate::music::MediaMusicTrack;
use serde::{Deserialize, Serialize};
use serde_json::json;

fn api_base() -> String {
    crate::config::external_base_url(
        "MYMEDIA_LISTENBRAINZ_API_BASE",
        "https://api.listenbrainz.org/1",
    )
}

#[derive(Debug, Deserialize)]
pub struct ValidateTokenApiResponse {
    #[serde(default)]
    pub code: Option<u16>,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub user_name: Option<String>,
    #[serde(default)]
    pub valid: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenValidationResult {
    pub valid: bool,
    pub username: Option<String>,
    pub message: String,
}

/// Valida o token de usuário do ListenBrainz
pub async fn validate_user_token(token: &str) -> Result<TokenValidationResult, String> {
    let clean_token = token.trim();
    if clean_token.is_empty() {
        return Ok(TokenValidationResult {
            valid: false,
            username: None,
            message: "Token não pode ser vazio".to_string(),
        });
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;

    let url = format!("{}/validate-token", api_base());
    let resp = client
        .get(&url)
        .header("Authorization", format!("Token {clean_token}"))
        .send()
        .await
        .map_err(|e| format!("erro de conexão com ListenBrainz: {e}"))?;

    let status = resp.status();
    let body: ValidateTokenApiResponse = resp
        .json()
        .await
        .map_err(|e| format!("erro ao ler resposta do ListenBrainz: {e}"))?;

    if status.is_success() && body.valid {
        Ok(TokenValidationResult {
            valid: true,
            username: body.user_name,
            message: "Token válido com sucesso".to_string(),
        })
    } else {
        Ok(TokenValidationResult {
            valid: false,
            username: None,
            message: if !body.message.is_empty() {
                body.message
            } else {
                format!("Token inválido (HTTP {})", status)
            },
        })
    }
}

/// Submete um listen ("scrobble") para a conta do ListenBrainz
pub async fn submit_single_listen(
    token: &str,
    track: &MediaMusicTrack,
    listened_at: i64,
) -> Result<(), String> {
    let clean_token = token.trim();
    if clean_token.is_empty() {
        return Err("Token do ListenBrainz não configurado".to_string());
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;

    let mut track_metadata = serde_json::Map::new();
    track_metadata.insert("artist_name".to_string(), json!(track.artist_name));
    track_metadata.insert("track_name".to_string(), json!(track.track_name));
    if let Some(ref rel) = track.release_name
        && !rel.trim().is_empty()
    {
        track_metadata.insert("release_name".to_string(), json!(rel.trim()));
    }

    let mut additional_info = serde_json::Map::new();
    additional_info.insert("listening_from".to_string(), json!("ZMydia"));
    additional_info.insert("submission_client".to_string(), json!("ZMydia"));
    if !track.recording_mbid.trim().is_empty() {
        additional_info.insert(
            "recording_mbid".to_string(),
            json!(track.recording_mbid.trim()),
        );
    }

    track_metadata.insert("additional_info".to_string(), json!(additional_info));

    let payload = json!({
        "listen_type": "single",
        "payload": [
            {
                "listened_at": listened_at,
                "track_metadata": track_metadata
            }
        ]
    });

    let url = format!("{}/submit-listens", api_base());
    let resp = client
        .post(&url)
        .header("Authorization", format!("Token {clean_token}"))
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("erro ao enviar scrobble para ListenBrainz: {e}"))?;

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    tracing::debug!("ListenBrainz submit status: {status}, body: {body}");

    if !status.is_success() {
        return Err(format!("ListenBrainz submit erro HTTP ({status}): {body}"));
    }

    Ok(())
}

/// Submete o status "Tocando Agora" (playing_now) para o ListenBrainz
pub async fn submit_playing_now(token: &str, track: &MediaMusicTrack) -> Result<(), String> {
    let clean_token = token.trim();
    if clean_token.is_empty() {
        return Err("Token do ListenBrainz não configurado".to_string());
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;

    let mut track_metadata = serde_json::Map::new();
    track_metadata.insert("artist_name".to_string(), json!(track.artist_name));
    track_metadata.insert("track_name".to_string(), json!(track.track_name));
    if let Some(ref rel) = track.release_name
        && !rel.trim().is_empty()
    {
        track_metadata.insert("release_name".to_string(), json!(rel.trim()));
    }

    let mut additional_info = serde_json::Map::new();
    additional_info.insert("listening_from".to_string(), json!("ZMydia"));
    additional_info.insert("submission_client".to_string(), json!("ZMydia"));
    if !track.recording_mbid.trim().is_empty() {
        additional_info.insert(
            "recording_mbid".to_string(),
            json!(track.recording_mbid.trim()),
        );
    }

    track_metadata.insert("additional_info".to_string(), json!(additional_info));

    let payload = json!({
        "listen_type": "playing_now",
        "payload": [
            {
                "track_metadata": track_metadata
            }
        ]
    });

    let url = format!("{}/submit-listens", api_base());
    let resp = client
        .post(&url)
        .header("Authorization", format!("Token {clean_token}"))
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("erro ao enviar playing_now para ListenBrainz: {e}"))?;

    if !resp.status().is_success() {
        let err_text = resp.text().await.unwrap_or_default();
        return Err(format!("ListenBrainz playing_now erro HTTP: {err_text}"));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_validate_token_response() {
        let json_str = r#"{
            "code": 200,
            "message": "Token valid.",
            "user_name": "example_user",
            "valid": true
        }"#;

        let parsed: ValidateTokenApiResponse = serde_json::from_str(json_str).unwrap();
        assert!(parsed.valid);
        assert_eq!(parsed.user_name.as_deref(), Some("example_user"));
    }
}
