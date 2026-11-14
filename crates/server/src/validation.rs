/// Validates identifiers persisted for channels and received through routes.
///
/// Channel identifiers are database identifiers, not filesystem paths. Keeping
/// this validation outside the retired sidecar module prevents metadata storage
/// concerns from leaking back into the runtime.
pub fn is_valid_channel_id(id: &str) -> bool {
    let trimmed = id.trim();
    !trimmed.is_empty()
        && trimmed.chars().count() <= 128
        && trimmed
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '@')
}

/// Parses an external HTTPS URL and restricts it to an explicit host list.
pub fn external_https_url(raw: &str, allowed_hosts: &[&str]) -> Option<reqwest::Url> {
    let url = reqwest::Url::parse(raw.trim()).ok()?;
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        return None;
    }
    let host = url.host_str()?.to_ascii_lowercase();
    allowed_hosts
        .iter()
        .any(|allowed| host == *allowed || host.ends_with(&format!(".{allowed}")))
        .then_some(url)
}

pub fn youtube_url(raw: &str) -> Option<reqwest::Url> {
    external_https_url(raw, &["youtube.com", "youtu.be", "youtube-nocookie.com"])
}

pub fn youtube_image_url(raw: &str) -> Option<reqwest::Url> {
    external_https_url(
        raw,
        &[
            "yt3.ggpht.com",
            "ggpht.com",
            "googleusercontent.com",
            "ytimg.com",
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_channel_ids_without_accepting_paths() {
        assert!(is_valid_channel_id("ch_mori_calliope"));
        assert!(is_valid_channel_id("@handle"));
        assert!(!is_valid_channel_id(""));
        assert!(!is_valid_channel_id("../../videos"));
        assert!(!is_valid_channel_id("channel/name"));
    }

    #[test]
    fn validates_external_urls_by_parser_and_exact_host_boundary() {
        assert!(youtube_url("https://www.youtube.com/watch?v=abc").is_some());
        assert!(youtube_url("https://youtu.be/abc").is_some());
        assert!(youtube_url("http://youtube.com/watch?v=abc").is_none());
        assert!(youtube_url("https://youtube.com.evil.test/watch?v=abc").is_none());
        assert!(youtube_url("https://evil-youtube.com/watch?v=abc").is_none());
        assert!(youtube_url("https://user@youtube.com/watch?v=abc").is_none());
    }
}
