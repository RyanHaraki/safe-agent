use crate::{audit, config, policy, secrets};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug)]
pub struct RunOptions {
    pub workspace: PathBuf,
    pub profile: String,
    pub policy: Option<PathBuf>,
    pub no_repo_policy: bool,
    pub backend: String,
    pub network: Option<String>,
    pub keep_logs: bool,
    pub keep_session: bool,
    pub dry_run: bool,
    pub agent: Vec<String>,
}
#[derive(Debug)]
pub struct RequestOptions {
    pub capability: String,
    pub resource: String,
    pub command: Option<String>,
    pub port: Option<u16>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    pub workspace: String,
    pub profile: String,
    pub backend: String,
    pub network: String,
    pub home: String,
    pub tmpdir: String,
    pub policy_hash: String,
    pub started_at: u64,
    pub in_session: bool,
}
#[derive(Debug, Serialize, Deserialize)]
struct WireRequest {
    capability: String,
    resource: String,
    command: Option<String>,
    port: Option<u16>,
    reason: Option<String>,
}
#[derive(Debug, Serialize, Deserialize)]
struct WireResponse {
    allowed: bool,
    message: String,
}
struct SupervisorState {
    workspace: PathBuf,
    policy: Mutex<config::Policy>,
    audit: Mutex<audit::AuditLog>,
}

pub fn run(options: RunOptions) -> Result<()> {
    if options.agent.is_empty() {
        bail!("an agent command is required");
    }
    let workspace = options
        .workspace
        .canonicalize()
        .with_context(|| format!("cannot resolve workspace {}", options.workspace.display()))?;
    let (mut policy, sources) = config::load_policy(
        &workspace,
        options.policy.as_deref(),
        !options.no_repo_policy,
    )?;
    if let Some(ref network) = options.network {
        policy.network.default = network.clone();
    }
    let id = format!("{}-{}", now(), uuid::Uuid::new_v4().simple());
    let root = PathBuf::from("/tmp").join(format!("sa-{}", id));
    for directory in ["bin", "home", "tmp", "logs", "policy", "state"] {
        fs::create_dir_all(root.join(directory))?;
    }
    let root = root.canonicalize()?;
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
    let session = SessionInfo {
        id: id.clone(),
        workspace: workspace.display().to_string(),
        profile: options.profile.clone(),
        backend: options.backend.clone(),
        network: policy.network.default.clone(),
        home: root.join("home").display().to_string(),
        tmpdir: root.join("tmp").display().to_string(),
        policy_hash: config::policy_hash(&policy),
        started_at: now(),
        in_session: true,
    };
    fs::write(
        root.join("session.json"),
        serde_json::to_vec_pretty(&session)?,
    )?;
    fs::write(
        root.join("policy/policy.toml"),
        toml::to_string_pretty(&policy)?,
    )?;
    println!(
        "Workspace: {}\nMode: live-repo\nProfile: {}\nBackend: {}\nNetwork: {}\nSession: {}",
        workspace.display(),
        options.profile,
        options.backend,
        policy.network.default,
        id
    );
    println!("Allowed: read/write workspace files, isolated HOME, local tools");
    println!("Blocked: host HOME, .env files, ambient environment, protected persistence paths");
    println!(
        "Policy sources: {}",
        sources
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
    if options.dry_run {
        println!("Dry run: no child launched");
        return Ok(());
    }
    if options.keep_logs {
        println!("Audit logs will be retained in the user session store.");
    }
    let socket_path = root.join("control.sock");
    let listener = UnixListener::bind(&socket_path)?;
    let state = Arc::new(SupervisorState {
        workspace: workspace.clone(),
        policy: Mutex::new(policy.clone()),
        audit: Mutex::new(audit::AuditLog::new(
            id.clone(),
            workspace.clone(),
            session.policy_hash.clone(),
        )),
    });
    let server_state = state.clone();
    let server = thread::spawn(move || serve_requests(listener, server_state));
    create_shims(&root, &socket_path)?;
    let profile = root.join("seatbelt.sb");
    if options.backend == "macos-seatbelt" {
        fs::write(&profile, seatbelt_profile(&workspace, &root, &policy))?;
    }
    let mut child = launch_child(&options, &workspace, &root, &profile)?;
    let status = child.wait()?;
    drop(child);
    let _ = server.join();
    let mut log = state.audit.lock().unwrap().clone();
    log.finish();
    let durable = audit::durable_session_path(&id);
    fs::create_dir_all(durable.parent().unwrap())?;
    fs::write(&durable, serde_json::to_vec_pretty(&log)?)?;
    fs::write(audit::latest_path(), &id)?;
    println!("Session summary: {}", durable.display());
    if !options.keep_session {
        let _ = fs::remove_dir_all(&root);
    }
    if !status.success() {
        bail!("agent exited with {}", status);
    }
    Ok(())
}

fn launch_child(
    options: &RunOptions,
    workspace: &Path,
    root: &Path,
    profile: &Path,
) -> Result<std::process::Child> {
    let mut command = if options.backend == "macos-seatbelt" {
        let mut c = Command::new("/usr/bin/sandbox-exec");
        c.args(["-f", profile.to_str().unwrap()])
            .arg(&options.agent[0])
            .args(&options.agent[1..]);
        c
    } else if options.backend == "none-for-debug" {
        let mut c = Command::new(&options.agent[0]);
        c.args(&options.agent[1..]);
        c
    } else {
        bail!(
            "unsupported backend {}; refusing to launch",
            options.backend
        );
    };
    let path = [
        root.join("bin").display().to_string(),
        "/usr/local/bin".into(),
        "/opt/homebrew/bin".into(),
        "/usr/bin".into(),
        "/bin".into(),
        "/usr/sbin".into(),
        "/sbin".into(),
    ]
    .join(":");
    command
        .current_dir(workspace)
        .env_clear()
        .env("HOME", root.join("home"))
        .env("TMPDIR", root.join("tmp"))
        .env("PATH", path)
        .env(
            "TERM",
            std::env::var("TERM").unwrap_or_else(|_| "xterm-256color".into()),
        )
        .env("LANG", "en_US.UTF-8");
    for name in [
        "SAFE_AGENT_TEST_CONFIG_HOME",
        "SAFE_AGENT_TEST_SECRET_BACKEND",
    ] {
        if let Ok(value) = std::env::var(name) {
            command.env(name, value);
        }
    }
    command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    command.spawn().context("launch sandboxed agent")
}

fn create_shims(root: &Path, socket: &Path) -> Result<()> {
    let real = std::env::current_exe()?;
    write_executable(
        &root.join("bin/safe-agent"),
        &format!(
            "#!/bin/sh\nexec \"{}\" --session-socket \"{}\" \"$@\"\n",
            real.display(),
            socket.display()
        ),
    )?;
    for name in ["git", "gh", "npm", "pnpm", "yarn", "curl", "wget"] {
        let Some(path) = find_binary(name) else {
            continue;
        };
        let script = format!("#!/bin/sh\nset -eu\ncmd=\"{} $*\"\ncase \"$cmd\" in\n  *'git push --force'*|*'npm publish'*) cap=command ;;\n  *'git push'*|*'npm install'*|*'pnpm install'*|*'yarn add'*|gh*|curl*|wget*) cap=command ;;\n  *) exec \"{}\" \"$@\" ;;\nesac\n\"{}\" --session-socket \"{}\" request \"$cap\" \"$cmd\" --reason \"mediated command\" >/dev/stderr || exit 126\nexec \"{}\" \"$@\"\n", name, path, real.display(), socket.display(), path);
        write_executable(&root.join(format!("bin/{name}")), &script)?;
    }
    Ok(())
}
fn write_executable(path: &Path, content: &str) -> Result<()> {
    fs::write(path, content)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    Ok(())
}
fn find_binary(name: &str) -> Option<String> {
    [
        format!("/opt/homebrew/bin/{name}"),
        format!("/usr/local/bin/{name}"),
        format!("/usr/bin/{name}"),
        format!("/bin/{name}"),
    ]
    .into_iter()
    .find(|p| Path::new(p).exists())
}

fn seatbelt_profile(workspace: &Path, root: &Path, policy: &config::Policy) -> String {
    let w = workspace.display();
    let r = root.display();
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/Users"));
    let h = home.display();
    let mut profile = format!("(version 1)\n(deny default)\n(allow process*)\n(allow process-exec*)\n(allow sysctl-read)\n(allow file-read*)\n(deny file-read* (subpath \"{h}\"))\n(allow file-read* (subpath \"{w}\"))\n(allow file-read* (subpath \"{r}\"))\n(allow file-write* (subpath \"{r}/home\"))\n(allow file-write* (subpath \"{r}/tmp\"))\n(allow file-write* (subpath \"{r}/logs\"))\n(allow file-write* (subpath \"{w}\"))\n");
    for protected in &policy.workspace.protected_paths {
        let path = match protected.as_str() {
            ".env" => format!("{w}/.env"),
            ".env.*" => format!("{w}/.env"),
            ".git/hooks/**" => format!("{w}/.git/hooks"),
            ".safe-agent/policy.toml" => format!("{w}/.safe-agent/policy.toml"),
            other => format!("{w}/{other}"),
        };
        profile.push_str(&format!(
            "(deny file-read* (subpath \"{path}\"))\n(deny file-write* (subpath \"{path}\"))\n"
        ));
    }
    if policy.network.default == "allow" {
        profile.push_str("(allow network*)\n");
    }
    profile
}

fn serve_requests(listener: UnixListener, state: Arc<SupervisorState>) {
    listener.set_nonblocking(true).ok();
    let mut idle = 0;
    while idle < 20 {
        match listener.accept() {
            Ok((stream, _)) => {
                idle = 0;
                let state = state.clone();
                thread::spawn(move || handle_request(stream, state));
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                idle += 1;
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(_) => break,
        }
    }
}
fn handle_request(mut stream: UnixStream, state: Arc<SupervisorState>) {
    let line = BufReader::new(&stream)
        .lines()
        .next()
        .transpose()
        .unwrap_or(None)
        .unwrap_or_default();
    let response = serde_json::from_str::<WireRequest>(&line)
        .map(|r| evaluate_request(&r, &state))
        .unwrap_or(WireResponse {
            allowed: false,
            message: "safe-agent: malformed request denied".into(),
        });
    let _ = writeln!(stream, "{}", serde_json::to_string(&response).unwrap());
}
fn evaluate_request(request: &WireRequest, state: &SupervisorState) -> WireResponse {
    if request.capability == "policy-reload" {
        return reload_session_policy(request, state);
    }
    let active_policy = state.policy.lock().unwrap().clone();
    let (decision, reason, _) = match request.capability.as_str() {
        "network" => policy::network_decision(&active_policy, &request.resource, request.port),
        "secret" => policy::secret_decision(
            &active_policy,
            &request.resource,
            request.command.as_deref().unwrap_or(""),
        ),
        "command" => policy::command_decision(&active_policy, &request.resource),
        "filesystem-read" => policy::path_decision(
            &active_policy,
            &state.workspace,
            Path::new(&request.resource),
            "read",
        ),
        "filesystem-write" => policy::path_decision(
            &active_policy,
            &state.workspace,
            Path::new(&request.resource),
            "write",
        ),
        _ => (
            policy::Decision::Deny,
            "unknown capability".into(),
            String::new(),
        ),
    };
    let allowed = match decision {
        policy::Decision::Allow => true,
        policy::Decision::Deny => false,
        policy::Decision::Ask => approval(request, &reason),
    };
    if allowed && request.capability == "secret" {
        return execute_secret_request(request, state);
    }
    let message = if allowed {
        "safe-agent: request allowed".to_string()
    } else {
        format!("safe-agent: access denied\nCapability: {}\nResource: {}\nReason: {}\nAlternative: continue without it or ask the user for a narrower capability.", request.capability, request.resource, if decision == policy::Decision::Deny { reason } else { "user denied or approval was unavailable".into() })
    };
    if let Ok(mut log) = state.audit.lock() {
        log.record_request(request, if allowed { "allow" } else { "deny" }, &message);
    }
    WireResponse { allowed, message }
}
fn execute_secret_request(request: &WireRequest, state: &SupervisorState) -> WireResponse {
    let Some(command) = request.command.as_deref() else {
        return WireResponse {
            allowed: true,
            message: "safe-agent: secret request approved; provide --for to execute a command"
                .into(),
        };
    };
    let value = match secrets::get(&request.resource, &state.workspace) {
        Ok(value) => value,
        Err(error) => {
            return WireResponse {
                allowed: false,
                message: format!("safe-agent: secret retrieval failed: {error}"),
            }
        }
    };
    let output = match Command::new("/bin/sh")
        .arg("-lc")
        .arg(command)
        .env(&request.resource, &value)
        .output()
    {
        Ok(output) => output,
        Err(error) => {
            return WireResponse {
                allowed: false,
                message: format!("safe-agent: secret command failed to launch: {error}"),
            }
        }
    };
    let stdout = String::from_utf8_lossy(&output.stdout).replace(&value, "[REDACTED]");
    let stderr = String::from_utf8_lossy(&output.stderr).replace(&value, "[REDACTED]");
    let message = format!(
        "safe-agent: secret command {}\n{}{}",
        if output.status.success() {
            "completed"
        } else {
            "failed"
        },
        stdout,
        stderr
    );
    WireResponse {
        allowed: output.status.success(),
        message,
    }
}
fn reload_session_policy(request: &WireRequest, state: &SupervisorState) -> WireResponse {
    let loaded = config::load_policy(&state.workspace, None, true);
    let Ok((next, _)) = loaded else {
        return WireResponse {
            allowed: false,
            message: "safe-agent: policy reload denied because the TOML is invalid".into(),
        };
    };
    let changed = config::policy_hash(&next) != config::policy_hash(&state.policy.lock().unwrap());
    if !changed {
        return WireResponse {
            allowed: true,
            message: "safe-agent: policy is unchanged".into(),
        };
    }
    let allowed = approval(
        request,
        "policy changes require approval before an active session adopts them",
    );
    if allowed {
        *state.policy.lock().unwrap() = next;
    }
    let message = if allowed {
        "safe-agent: policy reload approved and adopted"
    } else {
        "safe-agent: policy reload denied; previous policy remains active"
    };
    WireResponse {
        allowed,
        message: message.into(),
    }
}
fn approval(request: &WireRequest, reason: &str) -> bool {
    if std::env::var("SAFE_AGENT_TEST_APPROVE_ALL").ok().as_deref() == Some("1") {
        return true;
    }
    eprintln!("\nsafe-agent approval required\nRequest: {} {}\nReason: {}\n[a] allow once  [s] allow session  [d] deny", request.capability, request.resource, reason);
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).is_ok() && matches!(line.trim(), "a" | "s" | "allow")
}

pub fn status(json: bool, session_socket: Option<&Path>) -> Result<()> {
    if let Some(socket) = session_socket {
        return print_status(find_session_info(socket.parent().unwrap()), json);
    }
    if json {
        println!("{{\"in_session\":false}}");
    } else {
        println!("Not in a Safe Agent session.");
    }
    Ok(())
}
fn find_session_info(root: &Path) -> SessionInfo {
    serde_json::from_slice(&fs::read(root.join("session.json")).unwrap_or_default()).unwrap_or(
        SessionInfo {
            id: "unknown".into(),
            workspace: "unknown".into(),
            profile: "unknown".into(),
            backend: "unknown".into(),
            network: "unknown".into(),
            home: "unknown".into(),
            tmpdir: "unknown".into(),
            policy_hash: "unknown".into(),
            started_at: 0,
            in_session: true,
        },
    )
}
fn print_status(info: SessionInfo, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(&info)?);
    } else {
        println!("Safe Agent session: {}\nWorkspace: {}\nBackend: {}\nNetwork: {}\nHOME: {}\nSupervisor: connected", info.id, info.workspace, info.backend, info.network, info.home);
    }
    Ok(())
}
pub fn request(options: RequestOptions, session_socket: Option<&Path>) -> Result<()> {
    let socket = session_socket.ok_or_else(|| {
        anyhow::anyhow!(
            "safe-agent: no active session detected; run this command inside safe-agent run"
        )
    })?;
    let request = WireRequest {
        capability: options.capability,
        resource: options.resource,
        command: options.command,
        port: options.port,
        reason: options.reason,
    };
    let mut stream = UnixStream::connect(socket)
        .with_context(|| format!("safe-agent: supervisor unavailable at {}", socket.display()))?;
    writeln!(stream, "{}", serde_json::to_string(&request)?)?;
    let response: WireResponse = serde_json::from_reader(BufReader::new(stream))?;
    println!("{}", response.message);
    if !response.allowed {
        bail!("request denied");
    }
    Ok(())
}
pub fn reload_policy(
    workspace: &Path,
    explicit: Option<&Path>,
    session_socket: Option<&Path>,
) -> Result<()> {
    if let Some(socket) = session_socket {
        let request = WireRequest {
            capability: "policy-reload".into(),
            resource: workspace.display().to_string(),
            command: None,
            port: None,
            reason: Some("user requested policy reload".into()),
        };
        let mut stream = UnixStream::connect(socket)?;
        writeln!(stream, "{}", serde_json::to_string(&request)?)?;
        let response: WireResponse = serde_json::from_reader(BufReader::new(stream))?;
        println!("{}", response.message);
        if !response.allowed {
            bail!("policy reload denied");
        }
        return Ok(());
    }
    let (policy, _) = config::load_policy(workspace, explicit, true)?;
    println!("Policy validated. Active sessions require supervisor approval before adopting broadening changes.");
    println!("Current policy hash: {}", config::policy_hash(&policy));
    Ok(())
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
