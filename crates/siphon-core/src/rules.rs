use crate::model::{FileCategory, TaskSource};
use chrono::Local;
use glob_match::glob_match;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum RuleCondition {
    Extensions(Vec<String>),
    DomainGlob(String),
    DomainRegex(String),
    MimePrefix(String),
    MinSizeBytes(u64),
    MaxSizeBytes(u64),
    Source(TaskSource),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuleAction {
    /// Directory template supporting `{base}`, `{date}`, `{domain}`, `{category}`, `{ext}`.
    pub save_dir_template: Option<String>,
    pub category_override: Option<FileCategory>,
    pub connections_override: Option<u32>,
    pub speed_limit_bps: Option<u64>,
    pub auto_start: Option<bool>,
    pub post_command: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationRule {
    pub id: Uuid,
    pub name: String,
    pub enabled: bool,
    pub priority: i32,
    pub stop_on_match: bool,
    /// When true, all conditions must match (AND); when false, any condition matches (OR).
    pub match_all: bool,
    pub conditions: Vec<RuleCondition>,
    pub action: RuleAction,
}

#[derive(Debug, Clone)]
pub struct RuleEvalInput<'a> {
    pub url: &'a str,
    pub domain: &'a str,
    pub filename: &'a str,
    pub mime: Option<&'a str>,
    pub size_bytes: Option<u64>,
    pub category: FileCategory,
    pub source: TaskSource,
    pub base_save_dir: &'a Path,
}

#[derive(Debug, Clone, Default)]
pub struct RuleMatchResult {
    pub matched_rules: Vec<String>,
    pub save_dir: Option<PathBuf>,
    pub category_override: Option<FileCategory>,
    pub connections_override: Option<u32>,
    pub speed_limit_bps: Option<u64>,
    pub auto_start: Option<bool>,
    pub post_commands: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct RuleEngine {
    rules: Vec<AutomationRule>,
}

impl RuleEngine {
    pub fn new(mut rules: Vec<AutomationRule>) -> Self {
        for r in &mut rules {
            if let Some(tpl) = &mut r.action.save_dir_template {
                *tpl = tpl.replace('/', &std::path::MAIN_SEPARATOR.to_string());
            }
        }
        rules.sort_by_key(|r| r.priority);
        Self { rules }
    }

    pub fn default_rules() -> Vec<AutomationRule> {
        vec![
            AutomationRule {
                id: Uuid::new_v4(),
                name: "系统镜像自动归档".to_string(),
                enabled: true,
                priority: 10,
                stop_on_match: true,
                match_all: false,
                conditions: vec![RuleCondition::Extensions(vec![
                    "iso".into(),
                    "img".into(),
                    "vhdx".into(),
                ])],
                action: RuleAction {
                    save_dir_template: Some(r"{base}\Images".into()),
                    category_override: Some(FileCategory::DiskImage),
                    connections_override: Some(12),
                    ..Default::default()
                },
            },
            AutomationRule {
                id: Uuid::new_v4(),
                name: "视频文件自动归档".to_string(),
                enabled: true,
                priority: 20,
                stop_on_match: true,
                match_all: false,
                conditions: vec![
                    RuleCondition::Extensions(vec![
                        "mp4".into(),
                        "mkv".into(),
                        "webm".into(),
                        "mov".into(),
                    ]),
                    RuleCondition::MimePrefix("video/".into()),
                ],
                action: RuleAction {
                    save_dir_template: Some(r"{base}\Videos".into()),
                    category_override: Some(FileCategory::Video),
                    ..Default::default()
                },
            },
            AutomationRule {
                id: Uuid::new_v4(),
                name: "压缩包自动归档".to_string(),
                enabled: true,
                priority: 30,
                stop_on_match: true,
                match_all: false,
                conditions: vec![RuleCondition::Extensions(vec![
                    "zip".into(),
                    "7z".into(),
                    "rar".into(),
                    "tar".into(),
                    "gz".into(),
                    "xz".into(),
                ])],
                action: RuleAction {
                    save_dir_template: Some(r"{base}\Archives".into()),
                    category_override: Some(FileCategory::Archive),
                    ..Default::default()
                },
            },
            AutomationRule {
                id: Uuid::new_v4(),
                name: "安装程序自动归档".to_string(),
                enabled: true,
                priority: 40,
                stop_on_match: true,
                match_all: false,
                conditions: vec![RuleCondition::Extensions(vec![
                    "exe".into(),
                    "msi".into(),
                    "msix".into(),
                ])],
                action: RuleAction {
                    save_dir_template: Some(r"{base}\Software".into()),
                    category_override: Some(FileCategory::Software),
                    ..Default::default()
                },
            },
        ]
    }

    pub fn rules(&self) -> &[AutomationRule] {
        &self.rules
    }

    pub fn set_rules(&mut self, mut rules: Vec<AutomationRule>) {
        for r in &mut rules {
            if let Some(tpl) = &mut r.action.save_dir_template {
                *tpl = tpl.replace('/', &std::path::MAIN_SEPARATOR.to_string());
            }
        }
        rules.sort_by_key(|r| r.priority);
        self.rules = rules;
    }

    pub fn evaluate(&self, input: &RuleEvalInput<'_>) -> RuleMatchResult {
        let mut result = RuleMatchResult::default();
        let ext = extract_ext(input.filename);

        for rule in &self.rules {
            if !rule.enabled || rule.conditions.is_empty() {
                continue;
            }
            let matched = if rule.match_all {
                rule.conditions
                    .iter()
                    .all(|c| condition_matches(c, input, &ext))
            } else {
                rule.conditions
                    .iter()
                    .any(|c| condition_matches(c, input, &ext))
            };

            if !matched {
                continue;
            }

            result.matched_rules.push(rule.name.clone());

            if let Some(tpl) = &rule.action.save_dir_template
                && result.save_dir.is_none()
            {
                result.save_dir = Some(expand_dir_template(tpl, input, &ext));
            }
            if rule.action.category_override.is_some() && result.category_override.is_none() {
                result.category_override = rule.action.category_override;
            }
            if rule.action.connections_override.is_some() && result.connections_override.is_none() {
                result.connections_override = rule.action.connections_override;
            }
            if rule.action.speed_limit_bps.is_some() && result.speed_limit_bps.is_none() {
                result.speed_limit_bps = rule.action.speed_limit_bps;
            }
            if rule.action.auto_start.is_some() && result.auto_start.is_none() {
                result.auto_start = rule.action.auto_start;
            }
            if let Some(cmd) = &rule.action.post_command {
                result.post_commands.push(cmd.clone());
            }

            if rule.stop_on_match {
                break;
            }
        }

        result
    }
}

fn extract_ext(filename: &str) -> String {
    let lower = filename.to_ascii_lowercase();
    lower
        .rsplit('.')
        .next()
        .filter(|e| *e != lower.as_str())
        .unwrap_or("")
        .to_string()
}

fn condition_matches(cond: &RuleCondition, input: &RuleEvalInput<'_>, ext: &str) -> bool {
    match cond {
        RuleCondition::Extensions(exts) => {
            !ext.is_empty()
                && exts
                    .iter()
                    .any(|e| e.trim_start_matches('.').eq_ignore_ascii_case(ext))
        }
        RuleCondition::DomainGlob(pattern) => glob_match(
            &pattern.to_ascii_lowercase(),
            &input.domain.to_ascii_lowercase(),
        ),
        RuleCondition::DomainRegex(pat) => Regex::new(pat)
            .map(|re| re.is_match(input.domain) || re.is_match(input.url))
            .unwrap_or(false),
        RuleCondition::MimePrefix(prefix) => input
            .mime
            .map(|m| m.to_ascii_lowercase().starts_with(&prefix.to_ascii_lowercase()))
            .unwrap_or(false),
        RuleCondition::MinSizeBytes(min) => input.size_bytes.is_some_and(|s| s >= *min),
        RuleCondition::MaxSizeBytes(max) => input.size_bytes.is_some_and(|s| s <= *max),
        RuleCondition::Source(src) => input.source == *src,
    }
}

fn expand_dir_template(tpl: &str, input: &RuleEvalInput<'_>, ext: &str) -> PathBuf {
    let sep = std::path::MAIN_SEPARATOR.to_string();
    let base_str = input
        .base_save_dir
        .to_string_lossy()
        .replace(['/', '\\'], &sep);
    let date_str = Local::now().format("%Y-%m-%d").to_string();
    let safe_domain = sanitize_path_segment(input.domain);
    let safe_ext = if ext.is_empty() {
        "misc".to_string()
    } else {
        sanitize_path_segment(ext)
    };

    let expanded = tpl
        .replace("{base}", &base_str)
        .replace("{date}", &date_str)
        .replace("{domain}", &safe_domain)
        .replace("{category}", input.category.folder_slug())
        .replace("{ext}", &safe_ext)
        .replace(['/', '\\'], &sep);

    let path = PathBuf::from(expanded);
    let combined = if path.is_absolute() {
        path
    } else {
        input.base_save_dir.join(path)
    };
    combined.components().collect()
}

fn sanitize_path_segment(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_rule_matches_iso_and_expands_template() {
        let engine = RuleEngine::new(RuleEngine::default_rules());
        let base = PathBuf::from(r"C:\Users\Test\Downloads");
        let input = RuleEvalInput {
            url: "https://mirrors.tuna.tsinghua.edu.cn/ubuntu.iso",
            domain: "mirrors.tuna.tsinghua.edu.cn",
            filename: "ubuntu-24.04.iso",
            mime: Some("application/octet-stream"),
            size_bytes: Some(4_000_000_000),
            category: FileCategory::DiskImage,
            source: TaskSource::Manual,
            base_save_dir: &base,
        };
        let res = engine.evaluate(&input);
        assert_eq!(res.matched_rules, vec!["系统镜像自动归档"]);
        assert_eq!(
            res.save_dir,
            Some(PathBuf::from(r"C:\Users\Test\Downloads\Images"))
        );
        assert_eq!(res.connections_override, Some(12));
    }

    #[test]
    fn domain_glob_and_template_variables_work() {
        let rule = AutomationRule {
            id: Uuid::new_v4(),
            name: "GitHub Releases".into(),
            enabled: true,
            priority: 1,
            stop_on_match: true,
            match_all: true,
            conditions: vec![
                RuleCondition::DomainGlob("*.github.com".into()),
                RuleCondition::Extensions(vec!["zip".into()]),
            ],
            action: RuleAction {
                save_dir_template: Some("{base}/{domain}/{ext}".into()),
                speed_limit_bps: Some(5_000_000),
                ..Default::default()
            },
        };
        let engine = RuleEngine::new(vec![rule]);
        let base = PathBuf::from(r"D:\Downloads");
        let res = engine.evaluate(&RuleEvalInput {
            url: "https://objects.github.com/release.zip",
            domain: "objects.github.com",
            filename: "release.zip",
            mime: None,
            size_bytes: None,
            category: FileCategory::Archive,
            source: TaskSource::QuickBar,
            base_save_dir: &base,
        });
        assert_eq!(res.matched_rules, vec!["GitHub Releases"]);
        assert_eq!(
            res.save_dir,
            Some(PathBuf::from(r"D:\Downloads\objects.github.com\zip"))
        );
        assert_eq!(res.speed_limit_bps, Some(5_000_000));
    }
}
