use crate::config::{self, Policy};
use anyhow::Result;
use std::{
    net::IpAddr,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub struct ExplainOptions {
    pub kind: String,
    pub resource: String,
    pub action: Option<String>,
    pub port: Option<u16>,
    pub command: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Ask,
    Deny,
}
impl Decision {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Ask => "ask",
            Self::Deny => "deny",
        }
    }
}

pub fn validate(workspace: &Path, explicit: Option<&Path>) -> Result<()> {
    let (policy, sources) = config::load_policy(workspace, explicit, true)?;
    println!("Policy valid");
    println!("Profile: {}", policy.profile);
    println!("Sources:");
    for source in sources {
        println!("  {}", source.display());
    }
    Ok(())
}

pub fn explain(workspace: &Path, opts: ExplainOptions) -> Result<()> {
    let (policy, sources) = config::load_policy(workspace, None, true)?;
    let (decision, reason, matched) = match opts.kind.as_str() {
        "path" => path_decision(
            &policy,
            workspace,
            Path::new(&opts.resource),
            opts.action.as_deref().unwrap_or("read"),
        ),
        "network" => network_decision(&policy, &opts.resource, opts.port),
        "secret" => secret_decision(
            &policy,
            &opts.resource,
            opts.command.as_deref().unwrap_or(""),
        ),
        "command" => command_decision(&policy, &opts.resource),
        other => (
            Decision::Deny,
            format!("unknown explanation kind {other}"),
            "input validation".into(),
        ),
    };
    println!("Decision: {}", decision.as_str());
    println!("Request: {} {}", opts.kind, opts.resource);
    println!("Reason: {reason}");
    println!("Matched rule: {matched}");
    println!(
        "Policy sources: {}",
        sources
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
    if decision == Decision::Deny {
        println!("Alternative: request a narrower capability or continue without it.");
    }
    Ok(())
}

pub fn path_decision(
    policy: &Policy,
    workspace: &Path,
    path: &Path,
    action: &str,
) -> (Decision, String, String) {
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace.join(path)
    };
    let protected = policy
        .workspace
        .protected_paths
        .iter()
        .any(|p| protected_match(&candidate, workspace, p));
    if protected {
        return (
            Decision::Deny,
            "path is protected by the built-in or project policy".into(),
            "workspace.protected_paths".into(),
        );
    }
    if candidate.starts_with(workspace) {
        return (
            Decision::Allow,
            format!("{action} is inside the live workspace"),
            "workspace root".into(),
        );
    }
    (
        Decision::Deny,
        "path is outside the workspace".into(),
        "workspace boundary".into(),
    )
}

fn protected_match(candidate: &Path, workspace: &Path, pattern: &str) -> bool {
    let relative = candidate
        .strip_prefix(workspace)
        .unwrap_or(candidate)
        .to_string_lossy();
    match pattern {
        ".env" => relative == ".env",
        ".env.*" => relative == ".env" || relative.starts_with(".env."),
        ".git/hooks/**" => relative.starts_with(".git/hooks/") || relative == ".git/hooks",
        ".safe-agent/policy.toml" => relative == ".safe-agent/policy.toml",
        value => relative == value || relative.starts_with(&format!("{value}/")),
    }
}

pub fn network_decision(
    policy: &Policy,
    host: &str,
    port: Option<u16>,
) -> (Decision, String, String) {
    let resource = port
        .map(|p| format!("{host}:{p}"))
        .unwrap_or_else(|| host.to_string());
    if dangerous_host(host) {
        return (
            Decision::Deny,
            "private, loopback, or cloud metadata destinations are denied".into(),
            "network.deny dangerous range".into(),
        );
    }
    if matches_rule(&policy.network.deny, &resource) || matches_rule(&policy.network.deny, host) {
        return (
            Decision::Deny,
            "destination matches an explicit deny rule".into(),
            "network.deny".into(),
        );
    }
    if matches_rule(&policy.network.allow, &resource) || matches_rule(&policy.network.allow, host) {
        return (
            Decision::Allow,
            "destination matches an explicit allow rule".into(),
            "network.allow".into(),
        );
    }
    if matches_rule(&policy.network.ask, &resource) || matches_rule(&policy.network.ask, host) {
        return (
            Decision::Ask,
            "destination matches an explicit ask rule".into(),
            "network.ask".into(),
        );
    }
    let decision = match policy.network.default.as_str() {
        "allow" => Decision::Allow,
        "deny" => Decision::Deny,
        _ => Decision::Ask,
    };
    (
        decision,
        format!(
            "no narrower destination rule matched; default network is {}",
            policy.network.default
        ),
        "network.default".into(),
    )
}

fn dangerous_host(host: &str) -> bool {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(v4) => {
                v4.is_loopback()
                    || v4.is_private()
                    || v4.is_link_local()
                    || v4.octets() == [169, 254, 169, 254]
            }
            IpAddr::V6(v6) => v6.is_loopback() || v6.is_unique_local(),
        };
    }
    host == "localhost" || host.ends_with(".local")
}

fn matches_rule(rules: &[String], resource: &str) -> bool {
    rules.iter().any(|rule| {
        rule == resource
            || rule == "*"
            || (rule.ends_with(":*") && resource.starts_with(rule.trim_end_matches('*')))
    })
}

pub fn secret_decision(policy: &Policy, name: &str, command: &str) -> (Decision, String, String) {
    let Some(secret) = policy.secrets.get(name) else {
        return (
            Decision::Deny,
            "secret is not declared by the project policy".into(),
            "secrets declaration".into(),
        );
    };
    if !secret.allowed_commands.is_empty() && !secret.allowed_commands.iter().any(|c| command == c)
    {
        return (
            Decision::Deny,
            "command is not in the secret's allowed_commands list".into(),
            "secrets.allowed_commands".into(),
        );
    }
    (
        Decision::Ask,
        "secret use is always approval-gated and is injected into one subprocess only".into(),
        "secret broker".into(),
    )
}

pub fn command_decision(policy: &Policy, command: &str) -> (Decision, String, String) {
    if policy
        .commands
        .deny
        .iter()
        .any(|rule| command.starts_with(rule))
    {
        return (
            Decision::Deny,
            "command matches an explicit deny rule".into(),
            "commands.deny".into(),
        );
    }
    if policy.commands.allow.iter().any(|rule| command == *rule) {
        return (
            Decision::Allow,
            "command matches an explicit allow rule".into(),
            "commands.allow".into(),
        );
    }
    if policy.commands.ask.iter().any(|rule| {
        command == *rule || command.starts_with(&format!("{rule} ")) || command.starts_with(rule)
    }) {
        return (
            Decision::Ask,
            "command matches an approval rule".into(),
            "commands.ask".into(),
        );
    }
    (
        Decision::Ask,
        "unknown commands require approval".into(),
        "command default".into(),
    )
}

pub fn diff(workspace: &Path, explicit: Option<&Path>) -> Result<()> {
    let current = config::read_raw_policy(workspace, explicit)?;
    let session_dir = std::env::var("SAFE_AGENT_SESSION_DIR")
        .ok()
        .map(PathBuf::from);
    let previous = session_dir
        .map(|p| p.join("policy/policy.toml"))
        .filter(|p| p.exists())
        .map(std::fs::read_to_string)
        .transpose()?
        .unwrap_or_default();
    if current == previous {
        println!("No policy changes detected.");
    } else {
        println!("Policy changed.");
        println!("Current policy bytes: {}", current.len());
        println!("Previous policy bytes: {}", previous.len());
    }
    Ok(())
}
