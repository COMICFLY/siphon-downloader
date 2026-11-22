use serde::Deserialize;
use std::time::Duration;

pub const GITHUB_OWNER: &str = "COMICFLY";
pub const GITHUB_REPO: &str = "siphon-downloader";
pub const GITHUB_USER_URL: &str = "https://github.com/COMICFLY";
pub const GITHUB_REPO_URL: &str = "https://github.com/COMICFLY/siphon-downloader";
pub const GITHUB_RELEASES_URL: &str = "https://github.com/COMICFLY/siphon-downloader/releases";

#[derive(Debug, Clone)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub has_update: bool,
    pub release_name: String,
    pub release_url: String,
    pub installer_url: Option<String>,
    pub release_notes: String,
    pub published_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    body: Option<String>,
    html_url: String,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

fn parse_semver(v: &str) -> (u64, u64, u64) {
    let clean = v
        .trim()
        .trim_start_matches(['v', 'V'])
        .split(['-', '+'])
        .next()
        .unwrap_or("");
    let mut parts = clean.split('.');
    let major = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let patch = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    (major, minor, patch)
}

pub fn is_newer_version(current: &str, latest: &str) -> bool {
    parse_semver(latest) > parse_semver(current)
}

pub async fn check_for_updates(
    current_version: &str,
    proxy_url: Option<&str>,
) -> std::result::Result<UpdateInfo, String> {
    let mut builder = reqwest::Client::builder()
        .user_agent(format!("Siphon/{current_version}"))
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(15));

    if let Some(proxy_str) = proxy_url.map(str::trim).filter(|s| !s.is_empty()) {
        let proxy = reqwest::Proxy::all(proxy_str)
            .map_err(|e| format!("代理配置无效 ({proxy_str}): {e}"))?;
        builder = builder.proxy(proxy);
    }

    let client = builder
        .build()
        .map_err(|e| format!("初始化 HTTP 客户端失败: {e}"))?;

    let api_url = format!(
        "https://api.github.com/repos/{GITHUB_OWNER}/{GITHUB_REPO}/releases/latest"
    );

    let resp = client
        .get(&api_url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("连接 GitHub 检查更新失败: {e}"))?;

    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(UpdateInfo {
            current_version: current_version.to_string(),
            latest_version: current_version.to_string(),
            has_update: false,
            release_name: format!("v{current_version}"),
            release_url: GITHUB_RELEASES_URL.to_string(),
            installer_url: None,
            release_notes: "仓库暂未发布更新版本。".to_string(),
            published_at: None,
        });
    }

    if !resp.status().is_success() {
        return Err(format!("GitHub API 返回异常状态码 HTTP {}", resp.status().as_u16()));
    }

    let release: GhRelease = resp
        .json()
        .await
        .map_err(|e| format!("解析 GitHub Release 数据失败: {e}"))?;

    let latest_clean = release
        .tag_name
        .trim()
        .trim_start_matches(['v', 'V'])
        .to_string();
    let has_update = is_newer_version(current_version, &latest_clean);
    let installer_url = release
        .assets
        .iter()
        .find(|a| {
            let low = a.name.to_ascii_lowercase();
            low.ends_with(".exe") && low.contains("setup")
        })
        .or_else(|| {
            release
                .assets
                .iter()
                .find(|a| a.name.to_ascii_lowercase().ends_with(".exe"))
        })
        .map(|a| a.browser_download_url.clone());

    Ok(UpdateInfo {
        current_version: current_version.to_string(),
        latest_version: latest_clean,
        has_update,
        release_name: release
            .name
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(release.tag_name),
        release_url: release.html_url,
        installer_url,
        release_notes: release.body.unwrap_or_default(),
        published_at: release.published_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_comparison_works() {
        assert!(is_newer_version("0.2.0", "v0.2.1"));
        assert!(is_newer_version("0.2.0", "0.3.0"));
        assert!(is_newer_version("0.2.0", "1.0.0"));
        assert!(!is_newer_version("0.2.0", "v0.2.0"));
        assert!(!is_newer_version("0.2.0", "0.1.9"));
    }
}
