//! Acceptance tests that drive the real binary, because the interesting
//! failures live in what the process prints, not in what a function returns.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn run_json(home: &Path) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_brimlimd"))
        .args(["--json", "--no-daemon"])
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("CODEX_HOME")
        .output()
        .expect("brimlimd should run");

    assert!(output.status.success(), "exit status {:?}", output.status);
    serde_json::from_slice(&output.stdout).expect("stdout should be one JSON document")
}

#[test]
fn a_machine_with_no_assistants_reports_an_empty_list_not_an_error() {
    let home = tempfile::tempdir().unwrap();
    let state = run_json(home.path());

    assert_eq!(state["schema"], 1);
    assert!(state["generated_at"].is_string());
    assert_eq!(
        state["providers"].as_array().unwrap().len(),
        0,
        "an assistant that was never installed is absent, not failing"
    );
}

#[test]
fn an_installed_assistant_with_no_credentials_asks_for_auth_without_a_number() {
    let home = tempfile::tempdir().unwrap();
    // A Claude config directory with nothing in it: installed, not usable.
    std::fs::create_dir_all(home.path().join(".claude/sessions")).unwrap();

    let state = run_json(home.path());
    let claude = &state["providers"][0];

    assert_eq!(claude["id"], "claude");
    assert_eq!(claude["status"], "needs_auth");
    assert!(
        claude["headline_percent"].is_null(),
        "no credentials must never mean a number"
    );
    assert_eq!(claude["windows"].as_array().unwrap().len(), 0);
    assert!(
        claude["message"].is_string(),
        "a non-ok status has to say why"
    );
    assert_eq!(claude["fidelity"], "official");
}

#[test]
fn a_recorded_codex_rollout_comes_back_as_an_official_reading() {
    let home = tempfile::tempdir().unwrap();
    let day = home.path().join(".codex/sessions/2026/09/19");
    std::fs::create_dir_all(&day).unwrap();

    // The recorded shape, with its clock moved to now: the fixture pins the
    // format, and the test stays honest about expiry instead of rotting.
    let recorded = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/codex-rollout.jsonl"),
    )
    .unwrap();
    let now = chrono::Utc::now();
    let refreshed = recorded
        .replace("2026-09-19T16:46:48.200Z", &now.to_rfc3339())
        .replace("1790081648", &(now.timestamp() + 2 * 24 * 3600).to_string());
    std::fs::write(
        day.join("rollout-2026-09-19T11-11-10-fixture.jsonl"),
        refreshed,
    )
    .unwrap();

    let state = run_json(home.path());
    let codex = &state["providers"][0];

    assert_eq!(codex["id"], "codex");
    assert_eq!(codex["status"], "ok");
    assert_eq!(codex["fidelity"], "official");
    assert_eq!(codex["headline_percent"], 0.76);
    assert_eq!(codex["windows"][0]["name"], "Weekly");
    assert!(codex["updated_at"].is_string());
}
