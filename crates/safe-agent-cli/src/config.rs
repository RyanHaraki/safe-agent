use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Policy {
    pub version: u32,
    pub profile: String,
    pub workspace: WorkspacePolicy,
    pub network: NetworkPolicy,
    pub commands: CommandPolicy,
    pub secrets: std::collections::BTreeMap<String, SecretPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspacePolicy {
    pub protected_paths: Vec<String>,
    pub writable_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NetworkPolicy {
    pub default: String,
    pub ask: Vec<String>,
    pub allow: Vec<String>,
    pub deny: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CommandPolicy {
    pub allow: Vec<String>,
    pub ask: Vec<String>,
    pub deny: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SecretPolicy {
    pub purpose: Option<String>,
    pub mode: Option<String>,
    pub allowed_commands: Vec<String>,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            version: 1,
            profile: "repo-coder".into(),
            workspace: WorkspacePolicy::default(),
            network: NetworkPolicy::default(),
            commands: CommandPolicy::default(),
            secrets: Default::default(),
        }
    }
}
impl Default for WorkspacePolicy {
    fn default() -> Self {
        Self {
            protected_paths: vec![
                ".env".into(),
                ".env.*".into(),
                ".git/hooks/**".into(),
                ".safe-agent/policy.toml".into(),
            ],
            writable_paths: vec![],
        }
    }
}
impl Default for NetworkPolicy {
    fn default() -> Self {
        Self {
            default: "ask".into(),
            ask: vec![],
            allow: vec![],
            deny: vec![
                "127.0.0.1:*".into(),
                "192.168.0.0/16".into(),
                "10.0.0.0/8".into(),
                "169.254.169.254".into(),
                "::1".into(),
            ],
        }
    }
}
impl Default for CommandPolicy {
    fn default() -> Self {
        Self {
            allow: vec!["git status".into(), "git diff".into(), "git log".into()],
            ask: vec![
                "git push".into(),
                "npm install".into(),
                "pnpm install".into(),
                "yarn add".into(),
                "gh".into(),
                "curl".into(),
                "wget".into(),
            ],
            deny: vec![
                "git push --force".into(),
                "npm publish".into(),
                "sudo".into(),
            ],
        }
    }
}

pub fn config_dir() -> PathBuf {
    if let Ok(path) = std::env::var("SAFE_AGENT_TEST_CONFIG_HOME") {
        return PathBuf::from(path);
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("safe-agent")
}
pub fn user_config_path() -> PathBuf {
    config_dir().join("config.toml")
}
pub fn secrets_path() -> PathBuf {
    config_dir().join("secrets.toml")
}
pub fn repo_policy_path(workspace: &Path) -> PathBuf {
    workspace.join(".safe-agent/policy.toml")
}

pub fn load_policy(
    workspace: &Path,
    explicit: Option<&Path>,
    include_repo: bool,
) -> Result<(Policy, Vec<PathBuf>)> {
    let mut policy = Policy::default();
    let mut sources = vec![PathBuf::from("built-in defaults")];
    let paths = [
        user_config_path(),
        explicit
            .map(PathBuf::from)
            .unwrap_or_else(|| repo_policy_path(workspace)),
    ];
    for (index, path) in paths.iter().enumerate() {
        if index == 1 && !include_repo && explicit.is_none() {
            continue;
        }
        if !path.exists() {
            continue;
        }
        let content =
            fs::read_to_string(path).with_context(|| format!("read policy {}", path.display()))?;
        let parsed: Policy = toml::from_str(&content)
            .with_context(|| format!("parse TOML policy {}", path.display()))?;
        merge(&mut policy, parsed);
        sources.push(path.clone());
    }
    validate_policy(&policy)?;
    Ok((policy, sources))
}

fn merge(base: &mut Policy, overlay: Policy) {
    if overlay.version != 1 {
        base.version = overlay.version;
    }
    if overlay.profile != "repo-coder" {
        base.profile = overlay.profile;
    }
    if !overlay.workspace.protected_paths.is_empty() {
        base.workspace
            .protected_paths
            .extend(overlay.workspace.protected_paths);
    }
    if !overlay.workspace.writable_paths.is_empty() {
        base.workspace
            .writable_paths
            .extend(overlay.workspace.writable_paths);
    }
    if overlay.network.default != "ask" {
        base.network.default = overlay.network.default;
    }
    base.network.ask.extend(overlay.network.ask);
    base.network.allow.extend(overlay.network.allow);
    base.network.deny.extend(overlay.network.deny);
    base.commands.allow.extend(overlay.commands.allow);
    base.commands.ask.extend(overlay.commands.ask);
    base.commands.deny.extend(overlay.commands.deny);
    base.secrets.extend(overlay.secrets);
}

pub fn validate_policy(policy: &Policy) -> Result<()> {
    if policy.version != 1 {
        bail!("unsupported policy version {}", policy.version);
    }
    if !["allow", "ask", "deny"].contains(&policy.network.default.as_str()) {
        bail!("network.default must be allow, ask, or deny");
    }
    for name in policy.secrets.keys() {
        if !valid_name(name) {
            bail!("invalid secret name {name}");
        }
    }
    Ok(())
}
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_uppercase() || c == '_')
}

pub fn init(workspace: &Path, force: bool) -> Result<()> {
    let workspace = workspace
        .canonicalize()
        .with_context(|| format!("cannot resolve workspace {}", workspace.display()))?;
    let dir = workspace.join(".safe-agent");
    let path = dir.join("policy.toml");
    if path.exists() && !force {
        bail!(
            "{} already exists; use --force to overwrite",
            path.display()
        );
    }
    fs::create_dir_all(&dir)?;
    let template = r#"version = 1
profile = "repo-coder"

[network]
default = "ask"
ask = []
allow = []
deny = ["127.0.0.1:*", "10.0.0.0/8", "192.168.0.0/16", "169.254.169.254"]

[workspace]
protected_paths = [".env", ".env.*", ".git/hooks/**", ".safe-agent/policy.toml"]
writable_paths = []

[commands]
allow = ["git status", "git diff", "git log"]
ask = ["git push", "npm install", "pnpm install", "yarn add", "gh", "curl", "wget"]
deny = ["git push --force", "npm publish", "sudo"]
"#;
    fs::write(&path, template)?;
    println!("Created {}", path.display());
    Ok(())
}

pub fn install_skill(workspace: &Path) -> Result<()> {
    let path = workspace.join(".safe-agent/skills/SKILL.md");
    fs::create_dir_all(path.parent().unwrap())?;
    fs::write(&path, "# Safe Agent\n\nThis session is sandboxed. Use `safe-agent status --json` to inspect the session and `safe-agent request` for capabilities. Denials are expected and should be handled without attempting bypasses.\n")?;
    println!("Installed {}", path.display());
    Ok(())
}

pub fn policy_hash(policy: &Policy) -> String {
    let bytes = toml::to_string(policy).unwrap_or_default();
    let digest = Sha256::digest(bytes.as_bytes());
    format!("{digest:x}")
}

pub fn read_raw_policy(workspace: &Path, explicit: Option<&Path>) -> Result<String> {
    let path = explicit
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_policy_path(workspace));
    if !path.exists() {
        return Ok(String::new());
    }
    Ok(fs::read_to_string(path)?)
}
