//! Pricing profiles.
//!
//! A pricing profile is a named set of pricing files. A profile is either a
//! single `<name>.json` file or a `<name>/` directory whose `*.json` files are
//! loaded in file-name order; later files override earlier ones. The active
//! profile plus any extra files are merged on top of the embedded defaults to
//! build the [`Pricing`] used by every report command.

use crate::billing::{Pricing, PricingLayer, BUILTIN_PRICING_FILES};
use crate::config::{self, AppConfig};
use anyhow::{anyhow, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// Pseudo profile that only uses the embedded default pricing files.
pub const BUILTIN_PROFILE: &str = "builtin";

/// One discoverable profile on disk, or the built-in default.
#[derive(Clone, Debug)]
pub struct ProfileSummary {
    pub name: String,
    /// `None` for the built-in profile, which has no files on disk.
    pub path: Option<PathBuf>,
    /// Files loaded for this profile, in increasing precedence order.
    pub files: Vec<PathBuf>,
    /// Embedded pricing files that are always active, for the built-in profile.
    pub embedded_files: usize,
}

impl ProfileSummary {
    pub fn file_count(&self) -> usize {
        self.files.len() + self.embedded_files
    }

    pub fn display_path(&self) -> String {
        match &self.path {
            Some(path) => path.display().to_string(),
            None => "built-in defaults".to_string(),
        }
    }
}

/// Everything needed to build the effective pricing for one CLI invocation.
#[derive(Clone, Debug)]
pub struct PricingSelection {
    /// `None` when no config home exists and `--pricing-dir` was not given.
    pub dir: Option<PathBuf>,
    pub profile: Option<String>,
    pub extra_files: Vec<PathBuf>,
    pub use_builtin: bool,
}

/// CLI overrides for pricing selection.
#[derive(Clone, Debug, Default)]
pub struct PricingOptions {
    pub dir: Option<PathBuf>,
    pub profile: Option<String>,
    pub files: Vec<PathBuf>,
    pub use_builtin: Option<bool>,
}

/// Where profiles live when nothing else is configured.
pub fn default_dir() -> Option<PathBuf> {
    config::default_pricing_dir()
}

/// The pricing directory in effect, or `None` when there is no config home and
/// no explicit `--pricing-dir`.
pub fn resolve_dir(config: &AppConfig, options: &PricingOptions) -> Result<Option<PathBuf>> {
    if let Some(dir) = options.dir.clone().or_else(|| config.pricing_dir.clone()) {
        return config::expand_home_path(dir).map(Some);
    }
    Ok(default_dir())
}

/// Error for commands that genuinely need a pricing directory.
pub fn require_dir(selection: &PricingSelection) -> Result<&Path> {
    selection.dir.as_deref().ok_or_else(|| {
        anyhow!(
            "no pricing profile directory is configured; set HOME, XDG_CONFIG_HOME, or \
             TOKENCHECK_CONFIG, or pass --pricing-dir"
        )
    })
}

/// Merge CLI flags with the saved configuration.
pub fn resolve(config: &AppConfig, options: &PricingOptions) -> Result<PricingSelection> {
    let dir = resolve_dir(config, options)?;
    let profile = match &options.profile {
        Some(profile) => normalize_profile(profile),
        None => config
            .pricing_profile
            .as_deref()
            .and_then(normalize_profile),
    };

    let mut extra_files = Vec::new();
    // Legacy single-file setting first, then the ordered list, then CLI flags:
    // later entries always win.
    if let Some(legacy) = &config.pricing_file {
        extra_files.push(config::expand_home_path(legacy.clone())?);
    }
    for file in &config.pricing_files {
        extra_files.push(config::expand_home_path(file.clone())?);
    }
    for file in &options.files {
        extra_files.push(config::expand_home_path(file.clone())?);
    }

    Ok(PricingSelection {
        dir,
        profile,
        extra_files,
        use_builtin: options.use_builtin.unwrap_or(config.pricing_builtin),
    })
}

pub fn normalize_profile(profile: &str) -> Option<String> {
    let profile = profile.trim();
    if profile.is_empty() || profile == BUILTIN_PROFILE {
        return None;
    }
    Some(profile.to_string())
}

/// Build the effective pricing: embedded defaults, then profile files, then
/// extra files.
pub fn load(selection: &PricingSelection) -> Result<Pricing> {
    let mut pricing = if selection.use_builtin {
        Pricing::builtin()?
    } else {
        Pricing::empty()
    };
    if let Some(profile) = &selection.profile {
        let dir = require_dir(selection)?;
        for file in files_for_profile(dir, profile)? {
            pricing.push_layer(PricingLayer::from_path(&file)?);
        }
    }
    for file in &selection.extra_files {
        if !file.exists() {
            return Err(anyhow!("pricing file not found: {}", file.display()));
        }
        pricing.push_layer(PricingLayer::from_path(file)?);
    }
    Ok(pricing)
}

/// The built-in profile, which is always available even without a config home.
pub fn builtin_profile_summary() -> ProfileSummary {
    ProfileSummary {
        name: BUILTIN_PROFILE.to_string(),
        path: None,
        files: Vec::new(),
        embedded_files: BUILTIN_PRICING_FILES.len(),
    }
}

/// Every profile available in `dir`, plus the built-in profile.
pub fn list_profiles(dir: &Path) -> Result<Vec<ProfileSummary>> {
    let mut profiles = vec![builtin_profile_summary()];
    if !dir.is_dir() {
        return Ok(profiles);
    }

    let mut entries = fs::read_dir(dir)
        .with_context(|| format!("read {}", dir.display()))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .with_context(|| format!("read {}", dir.display()))?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("inspect {}", path.display()))?;
        if file_type.is_dir() {
            let files = json_files_in(&path)?;
            if files.is_empty() {
                continue;
            }
            profiles.push(ProfileSummary {
                name: entry.file_name().to_string_lossy().to_string(),
                path: Some(path),
                files,
                embedded_files: 0,
            });
        } else if is_json(&path) {
            let Some(stem) = path.file_stem() else {
                continue;
            };
            profiles.push(ProfileSummary {
                name: stem.to_string_lossy().to_string(),
                path: Some(path.clone()),
                files: vec![path],
                embedded_files: 0,
            });
        }
    }
    Ok(profiles)
}

/// Files loaded for `profile`, in increasing precedence order.
pub fn files_for_profile(dir: &Path, profile: &str) -> Result<Vec<PathBuf>> {
    let profiles = list_profiles(dir)?;
    if let Some(found) = profiles.iter().find(|entry| entry.name == profile) {
        return Ok(found.files.clone());
    }
    let available = profiles
        .iter()
        .map(|entry| entry.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    Err(anyhow!(
        "unknown pricing profile {profile:?} in {}; available profiles: {available}. Run `tkc pricing list`",
        dir.display()
    ))
}

/// Materialize the embedded pricing files into `<dir>/default/`.
pub fn init_profile_dir(dir: &Path, force: bool) -> Result<InitOutcome> {
    let profile_dir = dir.join("default");
    fs::create_dir_all(&profile_dir)
        .with_context(|| format!("create {}", profile_dir.display()))?;

    let mut outcome = InitOutcome {
        profile: "default".to_string(),
        dir: profile_dir.clone(),
        written: Vec::new(),
        skipped: Vec::new(),
    };
    for (name, content) in BUILTIN_PRICING_FILES {
        let path = profile_dir.join(name);
        if path.exists() && !force {
            outcome.skipped.push(path);
            continue;
        }
        fs::write(&path, content).with_context(|| format!("write {}", path.display()))?;
        outcome.written.push(path);
    }
    Ok(outcome)
}

#[derive(Clone, Debug)]
pub struct InitOutcome {
    pub profile: String,
    pub dir: PathBuf,
    pub written: Vec<PathBuf>,
    pub skipped: Vec<PathBuf>,
}

fn json_files_in(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
        let entry = entry.with_context(|| format!("read {}", dir.display()))?;
        let path = entry.path();
        if path.is_file() && is_json(&path) {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

fn is_json(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
}

#[cfg(test)]
mod tests {
    use super::{
        files_for_profile, init_profile_dir, list_profiles, load, PricingSelection, BUILTIN_PROFILE,
    };
    use crate::billing::Pricing;
    use crate::model::Source;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("tokencheck-{label}-{unique}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn lists_file_and_directory_profiles_in_name_order() {
        let dir = temp_dir("profiles");
        fs::write(
            dir.join("personal.json"),
            r#"{ "models": { "proxy-model": { "input": 1.0, "output": 2.0 } } }"#,
        )
        .unwrap();
        fs::create_dir_all(dir.join("team")).unwrap();
        fs::write(dir.join("team/20-second.json"), r#"{ "models": {} }"#).unwrap();
        fs::write(dir.join("team/10-first.json"), r#"{ "models": {} }"#).unwrap();
        fs::write(dir.join("notes.txt"), "ignored").unwrap();

        let profiles = list_profiles(&dir).unwrap();
        let names = profiles
            .iter()
            .map(|profile| profile.name.as_str())
            .collect::<Vec<_>>();

        assert_eq!(names, vec![BUILTIN_PROFILE, "personal", "team"]);
        let team = files_for_profile(&dir, "team").unwrap();
        assert_eq!(
            team,
            vec![
                dir.join("team/10-first.json"),
                dir.join("team/20-second.json")
            ]
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn directory_profile_layers_same_model_per_source() {
        let dir = temp_dir("layers");
        fs::create_dir_all(dir.join("mixed")).unwrap();
        fs::write(
            dir.join("mixed/10-official.json"),
            r#"{ "models": { "gpt-6": { "input": 10.0, "output": 20.0 } } }"#,
        )
        .unwrap();
        fs::write(
            dir.join("mixed/20-proxy.json"),
            r#"{
                "sources": ["claude"],
                "models": { "gpt-6": { "input": 1.0, "output": 2.0 } }
            }"#,
        )
        .unwrap();

        let pricing = load(&PricingSelection {
            dir: Some(dir.clone()),
            profile: Some("mixed".to_string()),
            extra_files: Vec::new(),
            use_builtin: false,
        })
        .unwrap();

        assert_eq!(pricing.layers().len(), 2);
        assert_eq!(
            pricing.lookup(Source::Claude, "gpt-6").unwrap().layer,
            dir.join("mixed/20-proxy.json").display().to_string()
        );
        assert_eq!(
            pricing.lookup(Source::Codex, "gpt-6").unwrap().layer,
            dir.join("mixed/10-official.json").display().to_string()
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn unknown_profiles_report_available_names() {
        let dir = temp_dir("unknown");
        fs::write(dir.join("known.json"), r#"{ "models": {} }"#).unwrap();

        let err = files_for_profile(&dir, "missing").unwrap_err().to_string();

        assert!(err.contains("unknown pricing profile"));
        assert!(err.contains("known"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn init_writes_editable_copies_of_the_embedded_pricing_files() {
        let dir = temp_dir("init");
        let outcome = init_profile_dir(&dir, false).unwrap();
        assert_eq!(outcome.profile, "default");
        assert!(outcome.written.len() >= 2);
        assert!(outcome.skipped.is_empty());

        let again = init_profile_dir(&dir, false).unwrap();
        assert!(again.written.is_empty());
        assert_eq!(again.skipped.len(), outcome.written.len());

        let forced = init_profile_dir(&dir, true).unwrap();
        assert_eq!(forced.written.len(), outcome.written.len());

        // The materialized profile must produce exactly the built-in pricing.
        let materialized = load(&PricingSelection {
            dir: Some(dir.clone()),
            profile: Some("default".to_string()),
            extra_files: Vec::new(),
            use_builtin: false,
        })
        .unwrap();
        assert_eq!(
            materialized.model_count(),
            Pricing::builtin().unwrap().model_count()
        );

        fs::remove_dir_all(&dir).unwrap();
    }
}
