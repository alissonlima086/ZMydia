use crate::music::MusicCandidate;
use serde::Deserialize;

const USER_AGENT: &str = "ZMydia/0.1.0 ( https://github.com/alissonlima086/ZMydia )";

fn api_base() -> String {
    crate::config::external_base_url(
        "MYMEDIA_MUSICBRAINZ_API_BASE",
        "https://musicbrainz.org/ws/2",
    )
}

#[derive(Debug, Deserialize)]
struct MbSearchResponse {
    #[serde(default)]
    recordings: Vec<MbRecording>,
}

#[derive(Debug, Deserialize)]
struct MbRecording {
    id: String,
    title: String,
    #[serde(default)]
    score: Option<u32>,
    #[serde(default)]
    length: Option<u32>,
    #[serde(default)]
    disambiguation: Option<String>,
    #[serde(rename = "first-release-date", default)]
    first_release_date: Option<String>,
    #[serde(rename = "artist-credit", default)]
    artist_credit: Vec<MbArtistCredit>,
    #[serde(default)]
    releases: Vec<MbRelease>,
}

#[derive(Debug, Deserialize)]
struct MbArtistCredit {
    name: String,
    #[serde(default)]
    joinphrase: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MbRelease {
    title: String,
    #[serde(default)]
    date: Option<String>,
    #[serde(rename = "release-group", default)]
    release_group: Option<MbReleaseGroup>,
}

#[derive(Debug, Deserialize)]
struct MbReleaseGroup {
    #[serde(rename = "primary-type", default)]
    primary_type: Option<String>,
}

fn format_artists(credits: &[MbArtistCredit]) -> String {
    if credits.is_empty() {
        return "Desconhecido".to_string();
    }
    let mut out = String::new();
    for credit in credits {
        out.push_str(&credit.name);
        if let Some(ref jp) = credit.joinphrase {
            out.push_str(jp);
        }
    }
    out
}

async fn fetch_musicbrainz_with_retry(
    build_req: impl Fn() -> reqwest::RequestBuilder,
) -> Result<reqwest::Response, String> {
    const MAX_RETRIES: u32 = 3;
    let mut attempt = 0;

    loop {
        attempt += 1;
        let req = build_req();
        let resp = req
            .send()
            .await
            .map_err(|e| format!("erro na requisição ao MusicBrainz: {e}"))?;

        let status = resp.status();
        if status == reqwest::StatusCode::SERVICE_UNAVAILABLE
            || status == reqwest::StatusCode::TOO_MANY_REQUESTS
        {
            if attempt <= MAX_RETRIES {
                let delay_secs = resp
                    .headers()
                    .get("Retry-After")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0);

                let sleep_ms = if delay_secs > 0 && delay_secs <= 5 {
                    delay_secs * 1000
                } else {
                    (attempt as u64) * 1200
                };

                tracing::warn!(
                    attempt,
                    %status,
                    sleep_ms,
                    "MusicBrainz is temporarily busy (shedding/rate limit); waiting to retry"
                );
                tokio::time::sleep(std::time::Duration::from_millis(sleep_ms)).await;
                continue;
            } else {
                return Err(format!("MusicBrainz HTTP {status}"));
            }
        }

        return Ok(resp);
    }
}

/// Busca gravações no MusicBrainz por termo de pesquisa (título / artista)
pub async fn search_musicbrainz(query: &str) -> Result<Vec<MusicCandidate>, String> {
    let clean_query = query.trim();
    if clean_query.is_empty() {
        return Ok(Vec::new());
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(12))
        .build()
        .map_err(|e| e.to_string())?;

    let url = format!("{}/recording", api_base());
    let resp = fetch_musicbrainz_with_retry(|| {
        client
            .get(&url)
            .header("User-Agent", USER_AGENT)
            .header("Accept", "application/json")
            .query(&[("query", clean_query), ("fmt", "json"), ("limit", "6")])
    })
    .await?;

    if !resp.status().is_success() {
        return Err(format!("MusicBrainz HTTP {}", resp.status()));
    }

    let mb_resp: MbSearchResponse = resp
        .json()
        .await
        .map_err(|e| format!("erro ao decodificar JSON do MusicBrainz: {e}"))?;

    let candidates = mb_resp
        .recordings
        .into_iter()
        .map(|rec| {
            let artist_name = format_artists(&rec.artist_credit);
            let release_name = rec.releases.first().map(|r| r.title.clone());
            let duration_secs = rec.length.map(|ms| ms / 1000);
            let score = rec.score.unwrap_or(50) as f32 / 100.0;
            let disambiguation = rec.disambiguation.and_then(|d| {
                let trimmed = d.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            });
            let release_date = rec
                .first_release_date
                .filter(|d| !d.trim().is_empty())
                .or_else(|| rec.releases.iter().find_map(|r| r.date.clone()));
            let release_type = rec.releases.iter().find_map(|r| {
                r.release_group
                    .as_ref()
                    .and_then(|rg| rg.primary_type.clone())
            });

            MusicCandidate {
                recording_mbid: rec.id,
                track_name: rec.title,
                artist_name,
                release_name,
                duration_secs,
                score,
                source: "musicbrainz".to_string(),
                disambiguation,
                release_date,
                release_type,
            }
        })
        .collect();

    Ok(candidates)
}

/// Consulta os detalhes de uma gravação pelo seu MusicBrainz Recording ID (MBID)
pub async fn lookup_recording_mbid(mbid: &str) -> Result<Option<MusicCandidate>, String> {
    let clean_mbid = mbid.trim();
    if clean_mbid.is_empty() {
        return Ok(None);
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(12))
        .build()
        .map_err(|e| e.to_string())?;

    let url = format!("{}/recording/{clean_mbid}", api_base());
    let resp = fetch_musicbrainz_with_retry(|| {
        client
            .get(&url)
            .header("User-Agent", USER_AGENT)
            .header("Accept", "application/json")
            .query(&[("inc", "artists+releases+release-groups"), ("fmt", "json")])
    })
    .await?;

    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }

    if !resp.status().is_success() {
        return Err(format!("MusicBrainz lookup HTTP {}", resp.status()));
    }

    let rec: MbRecording = resp
        .json()
        .await
        .map_err(|e| format!("erro ao decodificar gravação do MusicBrainz: {e}"))?;

    let artist_name = format_artists(&rec.artist_credit);
    let release_name = rec.releases.first().map(|r| r.title.clone());
    let duration_secs = rec.length.map(|ms| ms / 1000);
    let disambiguation = rec.disambiguation.and_then(|d| {
        let trimmed = d.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    });
    let release_date = rec
        .first_release_date
        .filter(|d| !d.trim().is_empty())
        .or_else(|| rec.releases.iter().find_map(|r| r.date.clone()));
    let release_type = rec.releases.iter().find_map(|r| {
        r.release_group
            .as_ref()
            .and_then(|rg| rg.primary_type.clone())
    });

    Ok(Some(MusicCandidate {
        recording_mbid: rec.id,
        track_name: rec.title,
        artist_name,
        release_name,
        duration_secs,
        score: 1.0,
        source: "manual_id".to_string(),
        disambiguation,
        release_date,
        release_type,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_artists_with_joinphrase() {
        let credits = vec![
            MbArtistCredit {
                name: "Example Artist".to_string(),
                joinphrase: Some(" feat. ".to_string()),
            },
            MbArtistCredit {
                name: "Guest Artist".to_string(),
                joinphrase: None,
            },
        ];
        assert_eq!(
            format_artists(&credits),
            "Example Artist feat. Guest Artist"
        );
    }

    #[test]
    fn test_parse_mb_search_response() {
        let json_str = r#"{
            "recordings": [
                {
                    "id": "00000000-0000-0000-0000-000000000001",
                    "score": 100,
                    "title": "Sample Track",
                    "length": 248000,
                    "disambiguation": "radio edit",
                    "first-release-date": "2020-01-02",
                    "artist-credit": [{"name": "Example Artist"}],
                    "releases": [
                        {
                            "title": "Sample Release",
                            "date": "2020-01-02",
                            "release-group": {
                                "primary-type": "Album"
                            }
                        }
                    ]
                }
            ]
        }"#;

        let parsed: MbSearchResponse = serde_json::from_str(json_str).unwrap();
        assert_eq!(parsed.recordings.len(), 1);
        assert_eq!(parsed.recordings[0].title, "Sample Track");
        assert_eq!(
            parsed.recordings[0].disambiguation.as_deref(),
            Some("radio edit")
        );
        assert_eq!(
            parsed.recordings[0].first_release_date.as_deref(),
            Some("2020-01-02")
        );
        assert_eq!(
            parsed.recordings[0].releases[0]
                .release_group
                .as_ref()
                .and_then(|rg| rg.primary_type.as_deref()),
            Some("Album")
        );
    }
}
