use std::{fs, process::Command};
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
