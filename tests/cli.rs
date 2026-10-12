//! End-to-end checks for how the built binary resolves its config file, pricing
//! profile directory, and home directory.
//!
//! These run the real binary with a scrubbed environment, so they also cover
//! the launchd/cron/container case where `HOME` is not set at all.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "tokencheck-cli-{label}-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// The binary under test, with every inherited environment variable removed.
fn tkc() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_tkc"));
    command.env_clear();
    command
}

fn run(command: &mut Command) -> Output {
    let output = command.output().expect("run tkc");
    assert!(
        output.status.success(),
        "tkc failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn write_pricing_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

#[test]
fn reports_and_pricing_work_without_any_environment() {
    let output = run(tkc().args(["pricing", "show"]));
    let text = stdout(&output);

    assert!(text.contains("Pricing directory: none"), "{text}");
    assert!(text.contains("Active profile: builtin"), "{text}");
    assert!(text.contains("Priced model entries:"), "{text}");
}

#[test]
fn prices_new_model_families_without_any_configuration() {
    let output = run(tkc().args(["pricing", "match", "gpt-5.6", "--source", "codex"]));
    let text = stdout(&output);

    assert!(text.contains("builtin:10-openai.json"), "{text}");
    // gpt-5.6 is an alias for gpt-5.6-sol: 4.00 in / 20.00 out.
    assert!(text.contains("$4"), "{text}");
}

#[test]
fn discovers_profiles_under_xdg_config_home() {
    let root = temp_dir("xdg");
    let pricing_dir = root.join("tokencheck").join("pricing").join("team");
    write_pricing_file(
        &pricing_dir.join("10-team.json"),
        r#"{ "models": { "gpt-5.6": { "input": 1.0, "cached_input": 0.1, "output": 2.0 } } }"#,
    );

    let listing = run(tkc()
        .env("XDG_CONFIG_HOME", &root)
        .args(["pricing", "list"]));
    let text = stdout(&listing);
    assert!(text.contains("team"), "{text}");
    assert!(text.contains(&pricing_dir.display().to_string()), "{text}");

    let matched = run(tkc().env("XDG_CONFIG_HOME", &root).args([
        "pricing",
        "match",
        "gpt-5.6",
        "--source",
        "codex",
        "--pricing-profile",
        "team",
    ]));
    let text = stdout(&matched);
    assert!(text.contains("10-team.json"), "{text}");
    assert!(text.contains("$1"), "{text}");

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn honors_the_tokencheck_config_override() {
    let root = temp_dir("config-override");
    let config_file = root.join("custom").join("config.json");
    fs::create_dir_all(config_file.parent().unwrap()).unwrap();
    fs::write(
        &config_file,
        format!(
            r#"{{"language":"en","pricing_dir":{:?},"pricing_profile":"work"}}"#,
            root.join("profiles").display().to_string()
        ),
    )
    .unwrap();
    write_pricing_file(
        &root.join("profiles").join("work").join("10-work.json"),
        r#"{ "models": { "gpt-6": { "input": 7.0, "output": 8.0 } } }"#,
    );

    let output = run(tkc()
        .env("TOKENCHECK_CONFIG", &config_file)
        .args(["pricing", "match", "gpt-6", "--source", "codex"]));
    let text = stdout(&output);

    assert!(text.contains("Active profile: work"), "{text}");
    assert!(text.contains("10-work.json"), "{text}");
    assert!(text.contains("$7"), "{text}");

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn pricing_directory_can_be_passed_explicitly() {
    let root = temp_dir("explicit-dir");
    write_pricing_file(
        &root.join("only").join("10-only.json"),
        r#"{ "models": { "proxy-model": { "input": 3.0, "output": 4.0 } } }"#,
    );

    let output = run(tkc().args([
        "pricing",
        "match",
        "proxy-model",
        "--source",
        "claude",
        "--pricing-dir",
        root.to_str().unwrap(),
        "--pricing-profile",
        "only",
        "--no-builtin-pricing",
    ]));
    let text = stdout(&output);

    assert!(text.contains("10-only.json"), "{text}");
    assert!(text.contains("$3"), "{text}");

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn fetch_accepts_an_explicit_home_without_the_environment() {
    let root = temp_dir("fetch-home");
    let data_file = root.join("out.json");

    let output = run(tkc().args([
        "fetch",
        "--home",
        root.to_str().unwrap(),
        "--data-file",
        data_file.to_str().unwrap(),
    ]));

    assert!(
        stdout(&output).contains("snapshot saved"),
        "{}",
        stdout(&output)
    );
    assert!(data_file.exists());

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn unknown_profiles_list_what_is_available() {
    let root = temp_dir("unknown-profile");
    write_pricing_file(
        &root.join("known.json"),
        r#"{ "models": { "m": { "input": 1.0, "output": 1.0 } } }"#,
    );

    let output = tkc()
        .args([
            "pricing",
            "match",
            "gpt-5.6",
            "--pricing-dir",
            root.to_str().unwrap(),
            "--pricing-profile",
            "missing",
        ])
        .output()
        .expect("run tkc");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown pricing profile"), "{stderr}");
    assert!(stderr.contains("known"), "{stderr}");

    fs::remove_dir_all(&root).unwrap();
}
