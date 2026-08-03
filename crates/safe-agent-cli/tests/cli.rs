use std::thread;
use std::time::Duration;
use std::{
    fs,
    process::{Command, Stdio},
};
use tempfile::TempDir;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_safe-agent")
}
fn run(args: &[&str]) -> std::process::Output {
    Command::new(bin())
        .args(args)
        .output()
        .expect("safe-agent should launch")
}
fn workspace() -> TempDir {
    tempfile::tempdir().expect("temp workspace")
}

#[test]
fn status_outside_session_reports_not_in_session() {
    let output = run(&["status", "--json"]);
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        r#"{"in_session":false}"#
    );
}

#[test]
fn init_writes_and_validates_toml_policy() {
    let dir = workspace();
    let path = dir.path().to_str().unwrap();
    let output = run(&["init", "--workspace", path]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let policy = dir.path().join(".safe-agent/policy.toml");
    assert!(policy.exists());
    assert!(String::from_utf8_lossy(&fs::read(&policy).unwrap()).contains("default = \"ask\""));
    let output = run(&["policy", "validate", "--workspace", path]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn debug_session_uses_fake_home_scrubs_env_and_allows_live_write() {
    let dir = workspace();
    let path = dir.path().to_str().unwrap();
    let command = "printf '%s|%s|%s' \"$HOME\" \"${SHOULD_NOT_LEAK-unset}\" \"$TMPDIR\"; printf ok > created.txt";
    let output = Command::new(bin())
        .env("SHOULD_NOT_LEAK", "secret-value")
        .args([
            "run",
            "--backend",
            "none-for-debug",
            "--workspace",
            path,
            "--",
            "/bin/sh",
            "-c",
            command,
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("/tmp/sa-"));
    assert!(stdout.contains("unset"));
    assert!(dir.path().join("created.txt").exists());
}

#[test]
fn policy_explain_denies_env_and_private_network() {
    let dir = workspace();
    fs::create_dir_all(dir.path().join(".safe-agent")).unwrap();
    fs::write(
        dir.path().join(".safe-agent/policy.toml"),
        "version = 1\n[network]\ndefault = \"ask\"\n",
    )
    .unwrap();
    let path = dir.path().to_str().unwrap();
    let output = run(&[
        "policy",
        "explain",
        "path",
        ".env",
        "--action",
        "read",
        "--workspace",
        path,
    ]);
    assert!(String::from_utf8_lossy(&output.stdout).contains("Decision: deny"));
    let output = run(&[
        "policy",
        "explain",
        "network",
        "169.254.169.254",
        "--workspace",
        path,
    ]);
    assert!(String::from_utf8_lossy(&output.stdout).contains("Decision: deny"));
}

#[test]
fn request_outside_session_is_denied() {
    let output = run(&["request", "network", "example.com", "--reason", "test"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no active session"));
}

#[cfg(target_os = "macos")]
#[test]
fn seatbelt_denies_repo_env_read_but_allows_normal_write() {
    let dir = workspace();
    fs::create_dir_all(dir.path().join(".safe-agent")).unwrap();
    fs::write(dir.path().join(".safe-agent/policy.toml"), "version = 1\n").unwrap();
    fs::write(dir.path().join(".env"), "TOP_SECRET=do-not-print\n").unwrap();
    let path = dir.path().to_str().unwrap();
    let command = "cat .env; printf ok > normal.txt";
    let output = Command::new(bin())
        .args(["run", "--workspace", path, "--", "/bin/sh", "-c", command])
        .output()
        .unwrap();
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!combined.contains("TOP_SECRET=do-not-print"));
    assert!(dir.path().join("normal.txt").exists());
}

#[test]
fn summary_has_durable_session_record() {
    let dir = workspace();
    let config = tempfile::tempdir().unwrap();
    let output = Command::new(bin())
        .env("SAFE_AGENT_TEST_CONFIG_HOME", config.path())
        .args([
            "run",
            "--backend",
            "none-for-debug",
            "--workspace",
            dir.path().to_str().unwrap(),
            "--",
            "/usr/bin/true",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let summary = Command::new(bin())
        .env("SAFE_AGENT_TEST_CONFIG_HOME", config.path())
        .args(["summary", "--json"])
        .output()
        .unwrap();
    assert!(summary.status.success());
    assert!(String::from_utf8_lossy(&summary.stdout).contains("session_id"));
    assert!(String::from_utf8_lossy(&summary.stdout).contains("changed_files"));
}

#[test]
fn secret_is_explicitly_requested_in_one_subprocess_and_redacted() {
    let dir = workspace();
    let config = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".safe-agent")).unwrap();
    fs::write(dir.path().join(".safe-agent/policy.toml"), "version = 1\n[secrets.TEST_SECRET]\nallowed_commands = [\"printf %s \\\"$TEST_SECRET\\\"\"]\n").unwrap();
    let envs = [
        (
            "SAFE_AGENT_TEST_CONFIG_HOME",
            config.path().to_str().unwrap(),
        ),
        ("SAFE_AGENT_TEST_SECRET_BACKEND", "memory"),
    ];
    let mut add = Command::new(bin());
    add.envs(envs.iter().copied()).args([
        "secrets",
        "add",
        "TEST_SECRET",
        "secret-value",
        "--backend",
        "test",
        "--project",
        dir.path().to_str().unwrap(),
    ]);
    let output = add.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut run = Command::new(bin());
    run.envs(envs.iter().copied())
        .env("SAFE_AGENT_TEST_APPROVE_ALL", "1")
        .args([
            "run",
            "--backend",
            "none-for-debug",
            "--workspace",
            dir.path().to_str().unwrap(),
            "--",
            "/bin/sh",
            "-c",
            "safe-agent request secret TEST_SECRET --for 'printf %s \"$TEST_SECRET\"'",
        ]);
    let output = run.output().unwrap();
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "{combined}");
    assert!(combined.contains("[REDACTED]"));
    assert!(!combined.contains("secret-value"));
}

#[test]
fn policy_reload_requires_approval_and_changes_the_active_decision() {
    let dir = workspace();
    let config = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".safe-agent")).unwrap();
    let policy = dir.path().join(".safe-agent/policy.toml");
    fs::write(
        &policy,
        "version = 1\n[network]\ndefault = \"deny\"\ndeny = [\"example.com\"]\n",
    )
    .unwrap();
    let marker = dir.path().join("first-request-complete");
    let command = format!("safe-agent request network example.com --reason first || true; printf done > '{}'; sleep 0.3; safe-agent request network example.com --reason second", marker.display());
    let mut command_builder = Command::new(bin());
    command_builder
        .env("SAFE_AGENT_TEST_CONFIG_HOME", config.path())
        .env("SAFE_AGENT_TEST_APPROVE_ALL", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .args([
            "run",
            "--backend",
            "none-for-debug",
            "--workspace",
            dir.path().to_str().unwrap(),
            "--",
            "/bin/sh",
            "-c",
            &command,
        ]);
    let child = command_builder.spawn().unwrap();
    for _ in 0..40 {
        if marker.exists() {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(marker.exists(), "first request did not complete");
    fs::write(&policy, "version = 1\n[network]\ndefault = \"allow\"\n").unwrap();
    let session_roots: Vec<_> = fs::read_dir("/tmp")
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry.file_name().to_string_lossy().starts_with("sa-")
                && entry.path().join("control.sock").exists()
        })
        .collect();
    let mut reloaded = false;
    for session_root in session_roots {
        let reload = Command::new(bin())
            .env("SAFE_AGENT_TEST_APPROVE_ALL", "1")
            .args([
                "--session-socket",
                session_root.path().join("control.sock").to_str().unwrap(),
                "policy",
                "reload",
                "--workspace",
                dir.path().to_str().unwrap(),
            ])
            .output()
            .unwrap();
        if reload.status.success()
            && String::from_utf8_lossy(&reload.stdout).contains("approved and adopted")
        {
            reloaded = true;
            break;
        }
    }
    assert!(reloaded, "active session policy was not reloaded");
    let output = child.wait_with_output().unwrap();
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(combined.contains("access denied"));
    assert!(combined.contains("request allowed"));
}

#[test]
fn quarantine_mode_edits_a_disposable_copy() {
    let dir = workspace();
    let output = Command::new(bin())
        .args([
            "run",
            "--backend",
            "none-for-debug",
            "--quarantine",
            "--workspace",
            dir.path().to_str().unwrap(),
            "--",
            "/bin/sh",
            "-c",
            "printf isolated > quarantine.txt",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!dir.path().join("quarantine.txt").exists());
    assert!(String::from_utf8_lossy(&output.stdout).contains("Mode: quarantine"));
}
