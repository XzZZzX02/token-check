use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Deserializer, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_DATA_FILE: &str = "data/tokencheck.json";
pub const DEFAULT_LIMIT: usize = 20;
pub const DEFAULT_HEATMAP_MONTHS: usize = 12;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct AppConfig {
    pub language: Language,
    pub source: SourcePreference,
    pub data_file: PathBuf,
    /// Named pricing profile; `builtin` (or empty) keeps the embedded defaults.
    #[serde(default, deserialize_with = "deserialize_optional_string")]
    pub pricing_profile: Option<String>,
    /// Directory that holds pricing profiles. `None` uses the default location.
    #[serde(default, deserialize_with = "deserialize_optional_path")]
    pub pricing_dir: Option<PathBuf>,
    /// Extra pricing files, loaded in order on top of the active profile.
    #[serde(default)]
    pub pricing_files: Vec<PathBuf>,
    /// Legacy single pricing file; kept for backward compatible configs.
    #[serde(default, deserialize_with = "deserialize_optional_path")]
    pub pricing_file: Option<PathBuf>,
    /// Whether the embedded default pricing files stay active.
    #[serde(default = "default_true")]
    pub pricing_builtin: bool,
    #[serde(default, deserialize_with = "deserialize_optional_path")]
    pub obsidian_snapshot_file: Option<PathBuf>,
    #[serde(default, deserialize_with = "deserialize_optional_path")]
    pub obsidian_dashboard_file: Option<PathBuf>,
    pub limit: usize,
    pub heatmap_months: usize,
}

fn default_true() -> bool {
    true
}

fn deserialize_optional_path<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<PathBuf>, D::Error>
where
    D: Deserializer<'de>,
{
    let path = Option::<PathBuf>::deserialize(deserializer)?;
    Ok(path.filter(|path| !path.as_os_str().is_empty()))
}

fn deserialize_optional_string<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    Ok(value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty()))
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            language: Language::default(),
            source: SourcePreference::default(),
            data_file: PathBuf::from(DEFAULT_DATA_FILE),
            pricing_profile: None,
            pricing_dir: None,
            pricing_files: Vec::new(),
            pricing_file: None,
            pricing_builtin: true,
            obsidian_snapshot_file: None,
            obsidian_dashboard_file: None,
            limit: DEFAULT_LIMIT,
            heatmap_months: DEFAULT_HEATMAP_MONTHS,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    En,
    Zh,
}

impl Language {
    pub fn as_str(self) -> &'static str {
        match self {
            Language::En => "en",
            Language::Zh => "zh",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Language::En => "English",
            Language::Zh => "中文",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "en" | "eng" | "english" | "e" | "英文" => Some(Language::En),
            "zh" | "cn" | "zho" | "chinese" | "中文" | "汉语" | "漢語" => Some(Language::Zh),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SourcePreference {
    #[default]
    All,
    Claude,
    Codex,
    OpenCode,
    Pi,
    #[serde(rename = "kimi")]
    KimiCode,
}

impl SourcePreference {
    pub fn as_str(self) -> &'static str {
        match self {
            SourcePreference::All => "all",
            SourcePreference::Claude => "claude",
            SourcePreference::Codex => "codex",
            SourcePreference::OpenCode => "opencode",
            SourcePreference::Pi => "pi",
            SourcePreference::KimiCode => "kimi",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "all" | "a" | "全部" | "所有" => Some(SourcePreference::All),
            "claude" | "c" => Some(SourcePreference::Claude),
            "codex" | "x" => Some(SourcePreference::Codex),
            "opencode" | "open-code" | "oc" | "o" => Some(SourcePreference::OpenCode),
            "pi" | "p" => Some(SourcePreference::Pi),
            "kimi" | "kimicode" | "kimi-code" | "kc" => Some(SourcePreference::KimiCode),
            _ => None,
        }
    }
}

/// Load the saved configuration, falling back to the built-in defaults when
/// there is no readable configuration at all.
///
/// A missing `HOME` is not an error here: report commands must keep working in
/// launchd agents, cron jobs, and containers, where `HOME` is often unset. They
/// simply run with the embedded default pricing.
pub fn load_or_default() -> Result<AppConfig> {
    match config_file_path() {
        Some(path) => load_or_default_from(&path),
        None => Ok(AppConfig::default()),
    }
}

pub fn save(config: &AppConfig) -> Result<PathBuf> {
    let path = config_path()?;
    save_to_path(&path, config)?;
    Ok(path)
}

pub fn reset() -> Result<(PathBuf, bool)> {
    let path = config_path()?;
    if path.exists() {
        fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
        return Ok((path, true));
    }
    Ok((path, false))
}

/// Directory that holds `config.json` and the pricing profile directory.
///
/// Resolution order: `TOKENCHECK_CONFIG` (its parent directory),
/// `XDG_CONFIG_HOME/tokencheck`, then `$HOME/.config/tokencheck`. Returns `None`
/// when none of them is available, which only happens in environments without a
/// home directory.
pub fn config_home() -> Option<PathBuf> {
    if let Some(path) = env_path("TOKENCHECK_CONFIG") {
        let parent = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        return Some(parent);
    }
    if let Some(dir) = env_path("XDG_CONFIG_HOME") {
        return Some(dir.join("tokencheck"));
    }
    env_path("HOME").map(|home| home.join(".config").join("tokencheck"))
}

fn config_file_path() -> Option<PathBuf> {
    if let Some(path) = env_path("TOKENCHECK_CONFIG") {
        return Some(path);
    }
    config_home().map(|dir| dir.join("config.json"))
}

pub fn config_path() -> Result<PathBuf> {
    config_file_path().ok_or_else(|| anyhow!("{NO_CONFIG_HOME}"))
}

/// Directory for pricing profiles, next to the config file.
///
/// `None` when no config home can be determined; callers that need a concrete
/// directory should ask the user for `--pricing-dir` in that case.
pub fn default_pricing_dir() -> Option<PathBuf> {
    config_home().map(|dir| dir.join("pricing"))
}

const NO_CONFIG_HOME: &str = "HOME is not set; set HOME, XDG_CONFIG_HOME, or TOKENCHECK_CONFIG";

fn env_path(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
}

/// Expand a leading `~` using the current user's home directory.
pub fn expand_home_path(path: PathBuf) -> Result<PathBuf> {
    let mut components = path.components();
    if let Some(std::path::Component::Normal(first)) = components.next() {
        if first == "~" {
            let mut expanded = home_dir()?;
            expanded.extend(components);
            return Ok(expanded);
        }
    }
    Ok(path)
}

fn normalize(config: &mut AppConfig) {
    if config.limit == 0 {
        config.limit = DEFAULT_LIMIT;
    }
    if config.heatmap_months == 0 {
        config.heatmap_months = DEFAULT_HEATMAP_MONTHS;
    }
    if config.data_file.as_os_str().is_empty() {
        config.data_file = PathBuf::from(DEFAULT_DATA_FILE);
    }
    config.pricing_profile = config
        .pricing_profile
        .take()
        .map(|profile| profile.trim().to_string())
        .filter(|profile| !profile.is_empty());
    config
        .pricing_files
        .retain(|path| !path.as_os_str().is_empty());
    if config
        .pricing_dir
        .as_ref()
        .is_some_and(|path| path.as_os_str().is_empty())
    {
        config.pricing_dir = None;
    }
    if config
        .pricing_file
        .as_ref()
        .is_some_and(|path| path.as_os_str().is_empty())
    {
        config.pricing_file = None;
    }
    if config
        .obsidian_snapshot_file
        .as_ref()
        .is_some_and(|path| path.as_os_str().is_empty())
    {
        config.obsidian_snapshot_file = None;
    }
    if config
        .obsidian_dashboard_file
        .as_ref()
        .is_some_and(|path| path.as_os_str().is_empty())
    {
        config.obsidian_dashboard_file = None;
    }
}

fn load_or_default_from(path: &Path) -> Result<AppConfig> {
    if !path.exists() {
        return Ok(AppConfig::default());
    }
    let content = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let mut config = serde_json::from_str::<AppConfig>(&content)
        .with_context(|| format!("parse {}", path.display()))?;
    normalize(&mut config);
    Ok(config)
}

fn save_to_path(path: &Path, config: &AppConfig) -> Result<()> {
    if config.limit == 0 {
        return Err(anyhow!("config limit must be greater than 0"));
    }
    if config.heatmap_months == 0 {
        return Err(anyhow!("config heatmap_months must be greater than 0"));
    }
    let mut config = config.clone();
    normalize(&mut config);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let mut content = serde_json::to_string_pretty(&config)?;
    content.push('\n');
    let tmp_path = path.with_extension("tmp");
    fs::write(&tmp_path, content).with_context(|| format!("write {}", tmp_path.display()))?;
    fs::rename(&tmp_path, path)
        .with_context(|| format!("replace {} with {}", path.display(), tmp_path.display()))?;
    Ok(())
}

fn home_dir() -> Result<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("HOME is not set; pass --home explicitly"))
}

#[cfg(test)]
mod tests {
    use super::{AppConfig, Language, SourcePreference, DEFAULT_DATA_FILE};

    #[test]
    fn defaults_are_stable() {
        let config = AppConfig::default();
        assert_eq!(config.language, Language::En);
        assert_eq!(config.source, SourcePreference::All);
        assert_eq!(config.data_file.to_string_lossy(), DEFAULT_DATA_FILE);
        assert_eq!(config.pricing_profile, None);
        assert_eq!(config.pricing_dir, None);
        assert!(config.pricing_files.is_empty());
        assert_eq!(config.pricing_file, None);
        assert!(config.pricing_builtin);
        assert_eq!(config.obsidian_snapshot_file, None);
        assert_eq!(config.obsidian_dashboard_file, None);
        assert_eq!(config.limit, 20);
        assert_eq!(config.heatmap_months, 12);
    }

    #[test]
    fn parses_language_aliases() {
        assert_eq!(Language::parse("en"), Some(Language::En));
        assert_eq!(Language::parse("中文"), Some(Language::Zh));
        assert_eq!(Language::parse("bad"), None);
    }

    #[test]
    fn parses_source_aliases() {
        assert_eq!(SourcePreference::parse("all"), Some(SourcePreference::All));
        assert_eq!(
            SourcePreference::parse("claude"),
            Some(SourcePreference::Claude)
        );
        assert_eq!(
            SourcePreference::parse("codex"),
            Some(SourcePreference::Codex)
        );
        assert_eq!(
            SourcePreference::parse("opencode"),
            Some(SourcePreference::OpenCode)
        );
        assert_eq!(SourcePreference::parse("pi"), Some(SourcePreference::Pi));
        assert_eq!(
            SourcePreference::parse("kimi"),
            Some(SourcePreference::KimiCode)
        );
        assert_eq!(
            SourcePreference::parse("kimi-code"),
            Some(SourcePreference::KimiCode)
        );
        assert_eq!(
            SourcePreference::parse("kimicode"),
            Some(SourcePreference::KimiCode)
        );
        assert_eq!(
            SourcePreference::parse("kc"),
            Some(SourcePreference::KimiCode)
        );
        assert_eq!(SourcePreference::parse("bad"), None);
    }

    #[test]
    fn missing_fields_use_project_defaults() {
        let config = serde_json::from_str::<AppConfig>(r#"{"language":"zh"}"#).unwrap();
        assert_eq!(config.language, Language::Zh);
        assert_eq!(config.source, SourcePreference::All);
        assert_eq!(config.data_file.to_string_lossy(), DEFAULT_DATA_FILE);
        assert_eq!(config.pricing_profile, None);
        assert_eq!(config.pricing_file, None);
        assert!(config.pricing_files.is_empty());
        assert!(config.pricing_builtin);
        assert_eq!(config.obsidian_snapshot_file, None);
        assert_eq!(config.obsidian_dashboard_file, None);
        assert_eq!(config.limit, 20);
        assert_eq!(config.heatmap_months, 12);
    }

    #[test]
    fn empty_pricing_file_uses_none() {
        let config =
            serde_json::from_str::<AppConfig>(r#"{"language":"zh","pricing_file":""}"#).unwrap();

        assert_eq!(config.pricing_file, None);
    }

    #[test]
    fn parses_pricing_profile_settings() {
        let config = serde_json::from_str::<AppConfig>(
            r#"{
                "pricing_profile": " personal ",
                "pricing_dir": "~/.tokencheck/pricing",
                "pricing_files": ["a.json", "", "b.json"],
                "pricing_builtin": false
            }"#,
        )
        .unwrap();

        assert_eq!(config.pricing_profile.as_deref(), Some("personal"));
        assert_eq!(
            config.pricing_dir,
            Some(std::path::PathBuf::from("~/.tokencheck/pricing"))
        );
        assert_eq!(config.pricing_files.len(), 3);
        assert!(!config.pricing_builtin);
    }

    #[test]
    fn empty_pricing_profile_uses_none() {
        let config =
            serde_json::from_str::<AppConfig>(r#"{"pricing_profile":"  ","pricing_dir":""}"#)
                .unwrap();

        assert_eq!(config.pricing_profile, None);
        assert_eq!(config.pricing_dir, None);
    }

    #[test]
    fn expands_leading_tilde_with_the_home_directory() {
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
        let expanded = super::expand_home_path(std::path::PathBuf::from("~/.tokencheck/p.json"));

        if let Some(home) = home {
            assert_eq!(expanded.unwrap(), home.join(".tokencheck").join("p.json"));
        }
        assert_eq!(
            super::expand_home_path(std::path::PathBuf::from("/abs/p.json")).unwrap(),
            std::path::PathBuf::from("/abs/p.json")
        );
    }

    #[test]
    fn empty_obsidian_paths_use_none() {
        let config = serde_json::from_str::<AppConfig>(
            r#"{"obsidian_snapshot_file":"","obsidian_dashboard_file":""}"#,
        )
        .unwrap();

        assert_eq!(config.obsidian_snapshot_file, None);
        assert_eq!(config.obsidian_dashboard_file, None);
    }
}
