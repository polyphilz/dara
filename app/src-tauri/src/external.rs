use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

#[tauri::command]
pub fn open_external_url(app: AppHandle, url: String) -> Result<(), String> {
    let parsed = parse_external_url(&url)?;

    app.opener()
        .open_url(parsed.as_str(), None::<&str>)
        .map_err(|error| format!("could not open external URL: {error}"))
}

fn parse_external_url(url: &str) -> Result<tauri::Url, String> {
    let parsed = tauri::Url::parse(url).map_err(|_| "invalid external URL".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err("only absolute HTTP(S) links can be opened".into());
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::parse_external_url;

    #[test]
    fn external_links_are_limited_to_absolute_http_urls() {
        assert!(parse_external_url("https://example.com/path").is_ok());
        assert!(parse_external_url("http://localhost:5173").is_ok());
        assert!(parse_external_url("/relative").is_err());
        assert!(parse_external_url("javascript:alert(1)").is_err());
        assert!(parse_external_url("file:///tmp/private").is_err());
        assert!(parse_external_url("tauri://invoke").is_err());
    }
}
