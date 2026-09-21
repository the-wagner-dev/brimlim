//! The daemon must never publish a blank state as if it were a reading.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Start a daemon on a private bus and ask for its state the moment the name
/// shows up — the race a frontend hits on every login.
#[test]
fn the_first_get_state_already_carries_a_poll() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".claude/sessions")).unwrap();

    let script = format!(
        r#"
        {daemon} &
        DAEMON=$!
        for _ in $(seq 1 100); do
            if busctl --user --list 2>/dev/null | grep -q org.brimlim.Daemon; then break; fi
            sleep 0.1
        done
        busctl --user --json=short call org.brimlim.Daemon /org/brimlim/Daemon \
            org.brimlim.Daemon GetState
        kill $DAEMON
        "#,
        daemon = env!("CARGO_BIN_EXE_brimlimd"),
    );

    let started = Instant::now();
    let output = Command::new("dbus-run-session")
        .args(["--", "bash", "-c", &script])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("XDG_STATE_HOME", home.path().join(".local/state"))
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("CODEX_HOME")
        .stderr(Stdio::null())
        .output()
        .expect("dbus-run-session should be available");

    assert!(
        started.elapsed() < Duration::from_secs(30),
        "daemon took too long to appear"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let reply: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("unexpected busctl output {stdout:?}: {e}"));

    // busctl --json=short wraps the reply as {"type":"s","data":["<json>"]}
    let payload = reply["data"][0]
        .as_str()
        .expect("GetState returns one string");
    let state: serde_json::Value = serde_json::from_str(payload).unwrap();

    assert_eq!(state["schema"], 1);
    assert_eq!(
        state["providers"].as_array().unwrap().len(),
        1,
        "the very first GetState must already reflect a poll, not an empty start-up state"
    );
    assert_eq!(state["providers"][0]["id"], "claude");
}
