use crate::{audit, config, policy, secrets, session};
use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "safe-agent",
    version,
    about = "Run coding agents with a local security boundary"
)]
pub struct Cli {
    #[arg(long, global = true, hide = true)]
    session_socket: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Run(RunArgs),
    Init(PathArgs),
    Status(StatusArgs),
    Request(RequestArgs),
    Policy(PolicyArgs),
    Secrets(SecretsArgs),
    Summary(SummaryArgs),
    Skills(SkillsArgs),
}

#[derive(Args, Debug)]
struct RunArgs {
    #[arg(short, long, default_value = ".")]
    workspace: PathBuf,
    #[arg(short, long, default_value = "repo-coder")]
    profile: String,
    #[arg(long)]
    policy: Option<PathBuf>,
    #[arg(long)]
    no_repo_policy: bool,
    #[arg(long, default_value = "macos-seatbelt")]
    backend: String,
    #[arg(long)]
    network: Option<String>,
    #[arg(long)]
    keep_logs: bool,
    #[arg(long)]
    keep_session: bool,
    #[arg(long)]
    dry_run: bool,
    #[arg(long)]
    quarantine: bool,
    #[arg(last = true, required = true)]
    agent: Vec<String>,
}

#[derive(Args, Debug)]
struct PathArgs {
    #[arg(short, long, default_value = ".")]
    workspace: PathBuf,
    #[arg(long)]
    force: bool,
}

#[derive(Args, Debug)]
struct StatusArgs {
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct RequestArgs {
    capability: String,
    resource: String,
    #[arg(long = "for", alias = "for-command")]
    for_command: Option<String>,
    #[arg(long)]
    port: Option<u16>,
    #[arg(long)]
    reason: Option<String>,
}

#[derive(Args, Debug)]
struct PolicyArgs {
    #[command(subcommand)]
    command: PolicyCommand,
}

#[derive(Subcommand, Debug)]
enum PolicyCommand {
    Validate(PathConfigArgs),
    Explain(ExplainArgs),
    Diff(PathConfigArgs),
    Reload(PathConfigArgs),
}

#[derive(Args, Debug)]
struct PathConfigArgs {
    #[arg(short, long, default_value = ".")]
    workspace: PathBuf,
    #[arg(long)]
    policy: Option<PathBuf>,
}

#[derive(Args, Debug)]
struct ExplainArgs {
    kind: String,
    resource: String,
    #[arg(long)]
    action: Option<String>,
    #[arg(long)]
    port: Option<u16>,
    #[arg(long)]
    for_command: Option<String>,
    #[arg(short, long, default_value = ".")]
    workspace: PathBuf,
}

#[derive(Args, Debug)]
struct SecretsArgs {
    #[command(subcommand)]
    command: SecretsCommand,
}

#[derive(Subcommand, Debug)]
enum SecretsCommand {
    Add(SecretAddArgs),
    Doctor(PathArgs),
}

#[derive(Args, Debug)]
struct SecretAddArgs {
    name: String,
    value: Option<String>,
    #[arg(long)]
    stdin: bool,
    #[arg(long)]
    project: Option<PathBuf>,
    #[arg(long, default_value = "keychain")]
    backend: String,
    #[arg(long)]
    overwrite: bool,
}

#[derive(Args, Debug)]
struct SummaryArgs {
    #[arg(long)]
    session: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct SkillsArgs {
    #[command(subcommand)]
    command: SkillsCommand,
}

#[derive(Subcommand, Debug)]
enum SkillsCommand {
    Install(PathArgs),
}

pub fn run(cli: Cli) -> Result<()> {
    let session_socket = cli.session_socket.as_deref();
    match cli.command {
        Command::Run(args) => session::run(args.into()),
        Command::Init(args) => config::init(&args.workspace, args.force),
        Command::Status(args) => session::status(args.json, session_socket),
        Command::Request(args) => session::request(args.into(), session_socket),
        Command::Policy(args) => match args.command {
            PolicyCommand::Validate(a) => policy::validate(&a.workspace, a.policy.as_deref()),
            PolicyCommand::Explain(a) => {
                let workspace = a.workspace.clone();
                policy::explain(&workspace, a.into())
            }
            PolicyCommand::Diff(a) => policy::diff(&a.workspace, a.policy.as_deref()),
            PolicyCommand::Reload(a) => {
                session::reload_policy(&a.workspace, a.policy.as_deref(), session_socket)
            }
        },
        Command::Secrets(args) => match args.command {
            SecretsCommand::Add(a) => secrets::add(a.into()),
            SecretsCommand::Doctor(a) => secrets::doctor(&a.workspace),
        },
        Command::Summary(args) => audit::summary(args.session.as_deref(), args.json),
        Command::Skills(args) => match args.command {
            SkillsCommand::Install(a) => config::install_skill(&a.workspace),
        },
    }
}

impl From<RunArgs> for session::RunOptions {
    fn from(a: RunArgs) -> Self {
        Self {
            workspace: a.workspace,
            profile: a.profile,
            policy: a.policy,
            no_repo_policy: a.no_repo_policy,
            backend: a.backend,
            network: a.network,
            keep_logs: a.keep_logs,
            keep_session: a.keep_session,
            dry_run: a.dry_run,
            quarantine: a.quarantine,
            agent: a.agent,
        }
    }
}
impl From<RequestArgs> for session::RequestOptions {
    fn from(a: RequestArgs) -> Self {
        Self {
            capability: a.capability,
            resource: a.resource,
            command: a.for_command,
            port: a.port,
            reason: a.reason,
        }
    }
}
impl From<ExplainArgs> for policy::ExplainOptions {
    fn from(a: ExplainArgs) -> Self {
        Self {
            kind: a.kind,
            resource: a.resource,
            action: a.action,
            port: a.port,
            command: a.for_command,
        }
    }
}
impl From<SecretAddArgs> for secrets::AddOptions {
    fn from(a: SecretAddArgs) -> Self {
        Self {
            name: a.name,
            value: a.value,
            stdin: a.stdin,
            project: a.project,
            backend: a.backend,
            overwrite: a.overwrite,
        }
    }
}
