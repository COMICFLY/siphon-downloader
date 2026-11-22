use crate::error::{Result, SiphonError};
use async_trait::async_trait;
use glob_match::glob_match;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveInput {
    pub url: String,
    pub headers: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExtraTaskSpec {
    pub url: String,
    pub filename: Option<String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResolveOutput {
    pub url: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    pub filename: Option<String>,
    #[serde(default)]
    pub extra_tasks: Vec<ExtraTaskSpec>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostDownloadInput {
    pub task_id: String,
    pub url: String,
    pub file_path: String,
    pub filename: String,
    pub total_bytes: u64,
    pub verified_hash: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PostDownloadOutput {
    pub ok: bool,
    pub note: Option<String>,
}

/// Compile-time Rust trait for custom URL / site resolvers.
#[async_trait]
pub trait LinkResolver: Send + Sync {
    fn name(&self) -> &str;
    fn matches(&self, url: &str) -> bool;
    async fn resolve(&self, input: &ResolveInput) -> Result<ResolveOutput>;
}

/// Compile-time Rust trait for post-download processing hooks.
#[async_trait]
pub trait PostProcessor: Send + Sync {
    fn name(&self) -> &str;
    async fn process(&self, input: &PostDownloadInput) -> Result<PostDownloadOutput>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub name: String,
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub hooks: Vec<String>,
    #[serde(default)]
    pub url_patterns: Vec<String>,
    pub command: Vec<String>,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

fn default_version() -> String {
    "0.1.0".to_string()
}

fn default_true() -> bool {
    true
}

fn default_timeout() -> u64 {
    15
}

#[derive(Debug, Clone)]
pub struct ExternalPlugin {
    pub manifest: PluginManifest,
    pub root_dir: PathBuf,
}

impl ExternalPlugin {
    pub fn has_hook(&self, hook: &str) -> bool {
        self.manifest.enabled && self.manifest.hooks.iter().any(|h| h == hook)
    }

    pub fn matches_url(&self, url: &str) -> bool {
        if self.manifest.url_patterns.is_empty() {
            return true;
        }
        let lower_url = url.to_ascii_lowercase();
        self.manifest
            .url_patterns
            .iter()
            .any(|p| glob_match(&p.to_ascii_lowercase(), &lower_url))
    }

    pub async fn run_json_hook<Req: Serialize, Resp: for<'de> Deserialize<'de>>(
        &self,
        req: &Req,
    ) -> Result<Resp> {
        let Some((program, args)) = self.manifest.command.split_first() else {
            return Err(SiphonError::Plugin {
                plugin: self.manifest.name.clone(),
                message: "command 配置为空".to_string(),
            });
        };

        let mut cmd = Command::new(program);
        cmd.args(args)
            .current_dir(&self.root_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        #[cfg(windows)]
        {
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = cmd.spawn().map_err(|e| SiphonError::Plugin {
            plugin: self.manifest.name.clone(),
            message: format!("启动插件进程失败: {e}"),
        })?;

        let payload = serde_json::to_vec(req)?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(&payload).await?;
            stdin.shutdown().await?;
        }

        let timeout = Duration::from_secs(self.manifest.timeout_secs.max(1));
        let output = tokio::time::timeout(timeout, child.wait_with_output())
            .await
            .map_err(|_| SiphonError::Plugin {
                plugin: self.manifest.name.clone(),
                message: format!("插件执行超时（>{}s）", self.manifest.timeout_secs),
            })?
            .map_err(|e| SiphonError::Plugin {
                plugin: self.manifest.name.clone(),
                message: format!("等待插件进程失败: {e}"),
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(SiphonError::Plugin {
                plugin: self.manifest.name.clone(),
                message: format!(
                    "退出码 {:?}: {}",
                    output.status.code(),
                    stderr.trim()
                ),
            });
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        serde_json::from_str::<Resp>(stdout.trim()).map_err(|e| SiphonError::Plugin {
            plugin: self.manifest.name.clone(),
            message: format!("插件返回的 JSON 无效: {e}"),
        })
    }
}

#[derive(Serialize)]
struct ExternalResolveReq<'a> {
    hook: &'static str,
    url: &'a str,
    headers: &'a BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct ExternalResolveResp {
    #[serde(default = "default_true")]
    ok: bool,
    url: Option<String>,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    filename: Option<String>,
    #[serde(default)]
    extra_tasks: Vec<ExtraTaskSpec>,
    note: Option<String>,
}

#[derive(Serialize)]
struct ExternalPostReq<'a> {
    hook: &'static str,
    task_id: &'a str,
    url: &'a str,
    file_path: &'a str,
    filename: &'a str,
    total_bytes: u64,
    verified_hash: Option<&'a str>,
}

pub struct PluginRegistry {
    search_dirs: Vec<PathBuf>,
    external_plugins: Vec<ExternalPlugin>,
    rust_resolvers: Vec<Arc<dyn LinkResolver>>,
    rust_post_processors: Vec<Arc<dyn PostProcessor>>,
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

impl PluginRegistry {
    pub fn new(search_dirs: Vec<PathBuf>) -> Self {
        let mut reg = Self {
            search_dirs,
            external_plugins: Vec::new(),
            rust_resolvers: Vec::new(),
            rust_post_processors: Vec::new(),
        };
        reg.reload();
        reg
    }

    pub fn register_resolver(&mut self, resolver: Arc<dyn LinkResolver>) {
        self.rust_resolvers.push(resolver);
    }

    pub fn register_post_processor(&mut self, processor: Arc<dyn PostProcessor>) {
        self.rust_post_processors.push(processor);
    }

    pub fn external_plugins(&self) -> &[ExternalPlugin] {
        &self.external_plugins
    }

    pub fn search_dirs(&self) -> &[PathBuf] {
        &self.search_dirs
    }

    pub fn reload(&mut self) {
        self.external_plugins.clear();
        let mut seen = std::collections::HashSet::new();

        for dir in &self.search_dirs {
            if !dir.is_dir() {
                continue;
            }
            let Ok(entries) = std::fs::read_dir(dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let plugin_dir = entry.path();
                if !plugin_dir.is_dir() {
                    continue;
                }
                let manifest_path = plugin_dir.join("plugin.toml");
                if !manifest_path.is_file() {
                    continue;
                }
                if let Ok(raw) = std::fs::read_to_string(&manifest_path) {
                    match toml::from_str::<PluginManifest>(&raw) {
                        Ok(manifest) => {
                            if seen.insert(manifest.name.clone()) {
                                self.external_plugins.push(ExternalPlugin {
                                    manifest,
                                    root_dir: plugin_dir,
                                });
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                path = %manifest_path.display(),
                                error = %e,
                                "解析插件 plugin.toml 失败"
                            );
                        }
                    }
                }
            }
        }
    }

    pub fn set_plugin_enabled(&mut self, name: &str, enabled: bool) -> Result<()> {
        if let Some(plugin) = self
            .external_plugins
            .iter_mut()
            .find(|p| p.manifest.name == name)
        {
            plugin.manifest.enabled = enabled;
            let manifest_path = plugin.root_dir.join("plugin.toml");
            let serialized = toml::to_string_pretty(&plugin.manifest)
                .map_err(|e| SiphonError::Config(e.to_string()))?;
            std::fs::write(manifest_path, serialized)?;
        }
        Ok(())
    }

    pub async fn run_resolve(&self, input: &ResolveInput) -> ResolveOutput {
        let mut current = ResolveOutput {
            url: input.url.clone(),
            headers: input.headers.clone(),
            filename: None,
            extra_tasks: Vec::new(),
            note: None,
        };

        for resolver in &self.rust_resolvers {
            if resolver.matches(&current.url) {
                let req = ResolveInput {
                    url: current.url.clone(),
                    headers: current.headers.clone(),
                };
                match resolver.resolve(&req).await {
                    Ok(out) => {
                        if !out.url.is_empty() {
                            current.url = out.url;
                        }
                        current.headers.extend(out.headers);
                        if out.filename.is_some() {
                            current.filename = out.filename;
                        }
                        current.extra_tasks.extend(out.extra_tasks);
                        if out.note.is_some() {
                            current.note = out.note;
                        }
                    }
                    Err(e) => {
                        tracing::warn!(resolver = resolver.name(), error = %e, "内置解析器失败，已跳过");
                    }
                }
            }
        }

        for plugin in &self.external_plugins {
            if !plugin.has_hook("resolve") || !plugin.matches_url(&current.url) {
                continue;
            }
            let req = ExternalResolveReq {
                hook: "resolve",
                url: &current.url,
                headers: &current.headers,
            };
            match plugin.run_json_hook::<_, ExternalResolveResp>(&req).await {
                Ok(resp) if resp.ok => {
                    if let Some(u) = resp.url
                        && !u.is_empty()
                    {
                        current.url = u;
                    }
                    current.headers.extend(resp.headers);
                    if resp.filename.is_some() {
                        current.filename = resp.filename;
                    }
                    current.extra_tasks.extend(resp.extra_tasks);
                    if resp.note.is_some() {
                        current.note = resp.note;
                    }
                }
                Ok(_) => {
                    tracing::warn!(plugin = %plugin.manifest.name, "插件 resolve 返回 ok=false");
                }
                Err(e) => {
                    tracing::warn!(plugin = %plugin.manifest.name, error = %e, "外部 resolve 插件失败，降级直连");
                }
            }
        }

        current
    }

    pub async fn run_post_download(&self, input: &PostDownloadInput) -> Vec<String> {
        let mut notes = Vec::new();

        for proc in &self.rust_post_processors {
            match proc.process(input).await {
                Ok(out) => {
                    if let Some(note) = out.note {
                        notes.push(format!("[{}] {}", proc.name(), note));
                    }
                }
                Err(e) => {
                    notes.push(format!("[{}] 执行失败: {}", proc.name(), e));
                }
            }
        }

        for plugin in &self.external_plugins {
            if !plugin.has_hook("post_download") || !plugin.matches_url(&input.url) {
                continue;
            }
            let req = ExternalPostReq {
                hook: "post_download",
                task_id: &input.task_id,
                url: &input.url,
                file_path: &input.file_path,
                filename: &input.filename,
                total_bytes: input.total_bytes,
                verified_hash: input.verified_hash.as_deref(),
            };
            match plugin.run_json_hook::<_, PostDownloadOutput>(&req).await {
                Ok(resp) => {
                    if let Some(note) = resp.note {
                        notes.push(format!("[{}] {}", plugin.manifest.name, note));
                    }
                }
                Err(e) => {
                    notes.push(format!("[{}] 执行失败: {}", plugin.manifest.name, e));
                }
            }
        }

        notes
    }
}

pub fn load_manifest_from_dir(dir: &Path) -> Option<PluginManifest> {
    let raw = std::fs::read_to_string(dir.join("plugin.toml")).ok()?;
    toml::from_str(&raw).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn external_plugin_bad_command_falls_back_safely() {
        let dir = tempfile::tempdir().unwrap();
        let plugin_dir = dir.path().join("broken");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        std::fs::write(
            plugin_dir.join("plugin.toml"),
            r#"
name = "broken"
enabled = true
hooks = ["resolve"]
url_patterns = ["*://example.com/*"]
command = ["nonexistent_siphon_binary_12345"]
timeout_secs = 2
"#,
        )
        .unwrap();

        let registry = PluginRegistry::new(vec![dir.path().to_path_buf()]);
        assert_eq!(registry.external_plugins().len(), 1);
        let out = registry
            .run_resolve(&ResolveInput {
                url: "https://example.com/file.zip".into(),
                headers: BTreeMap::new(),
            })
            .await;
        assert_eq!(out.url, "https://example.com/file.zip");
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn external_powershell_resolve_plugin_injects_header() {
        let dir = tempfile::tempdir().unwrap();
        let plugin_dir = dir.path().join("ps-test");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        std::fs::write(
            plugin_dir.join("plugin.toml"),
            r#"
name = "ps-test"
enabled = true
hooks = ["resolve"]
url_patterns = ["*://example.org/*"]
command = ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "run.ps1"]
timeout_secs = 15
"#,
        )
        .unwrap();
        std::fs::write(
            plugin_dir.join("run.ps1"),
            r#"
$raw = [Console]::In.ReadToEnd()
$req = $raw | ConvertFrom-Json
@{ ok = $true; url = "$($req.url)?resolved=1"; headers = @{ "X-Custom" = "siphon-ok" }; filename = "custom.zip" } | ConvertTo-Json -Compress
"#,
        )
        .unwrap();

        let registry = PluginRegistry::new(vec![dir.path().to_path_buf()]);
        let out = registry
            .run_resolve(&ResolveInput {
                url: "https://example.org/item".into(),
                headers: BTreeMap::new(),
            })
            .await;
        assert_eq!(out.url, "https://example.org/item?resolved=1");
        assert_eq!(
            out.headers.get("X-Custom").map(String::as_str),
            Some("siphon-ok")
        );
        assert_eq!(out.filename.as_deref(), Some("custom.zip"));
    }
}
