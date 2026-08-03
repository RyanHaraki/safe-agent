use crate::config;
use anyhow::{bail, Context, Result};
use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug)]
pub struct AddOptions {
    pub name: String,
    pub value: Option<String>,
    pub stdin: bool,
    pub project: Option<PathBuf>,
    pub backend: String,
    pub overwrite: bool,
}

pub fn add(options: AddOptions) -> Result<()> {
    if !config::valid_name(&options.name) {
        bail!("invalid secret name; use uppercase letters, digits, and underscores");
    }
    if std::env::args().any(|a| a == "--session-socket")
        && std::env::var("SAFE_AGENT_TEST_APPROVE_ALL").ok().as_deref() != Some("1")
    {
        bail!(
            "safe-agent: adding or changing a secret from an agent session requires user approval"
        );
    }
    let positional_value = options.value.is_some();
    let mut value = options.value.unwrap_or_default();
    if options.stdin {
        value.clear();
        io::stdin().read_to_string(&mut value)?;
    }
    if value.is_empty() {
        bail!("secret value is empty; provide VALUE, --stdin, or use a future hidden prompt");
    }
    let project = options
        .project
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let mapping = format!(
        "keychain://safe-agent/{}/{}",
        project.display(),
        options.name
    );
    let path = config::secrets_path();
    fs::create_dir_all(path.parent().unwrap())?;
    if path.exists() && !options.overwrite {
        let existing = fs::read_to_string(&path)?;
        if existing.contains(&format!("{} = ", options.name)) {
            bail!("secret mapping already exists; use --overwrite");
        }
    }
    if options.backend == "test"
        || std::env::var("SAFE_AGENT_TEST_SECRET_BACKEND")
            .ok()
            .as_deref()
            == Some("memory")
    {
        let store = path.with_extension("values");
        let mut content = if store.exists() {
            fs::read_to_string(&store)?
        } else {
            String::new()
        };
        content.push_str(&format!("{}={}\n", options.name, value.trim_end()));
        fs::write(store, content)?;
    } else {
        let service = format!("safe-agent/{}", project.display());
        let mut command = Command::new("security");
        command.args([
            "add-generic-password",
            "-a",
            &options.name,
            "-s",
            &service,
            "-w",
            &value,
        ]);
        if options.overwrite {
            command.arg("-U");
        }
        let output = command
            .output()
            .context("macOS Keychain is unavailable; use --backend test only for tests")?;
        if !output.status.success() {
            bail!(
                "Keychain write failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
    let mut mappings: toml::Value = if path.exists() {
        toml::from_str(&fs::read_to_string(&path)?)?
    } else {
        toml::Value::Table(Default::default())
    };
    let root = mappings.as_table_mut().expect("TOML root is a table");
    let projects = root
        .entry("projects")
        .or_insert_with(|| toml::Value::Table(Default::default()))
        .as_table_mut()
        .expect("projects is a table");
    let project_table = projects
        .entry(project.display().to_string())
        .or_insert_with(|| toml::Value::Table(Default::default()))
        .as_table_mut()
        .expect("project mapping is a table");
    project_table.insert(options.name.clone(), toml::Value::String(mapping));
    fs::write(&path, toml::to_string_pretty(&mappings)?)?;
    println!(
        "Stored secret {} in {} (value not printed)",
        options.name,
        if options.backend == "test" {
            "test backend"
        } else {
            "macOS Keychain"
        }
    );
    if positional_value {
        println!(
            "Warning: positional secret values can leak through shell history or process listings."
        );
    }
    Ok(())
}

pub fn doctor(workspace: &Path) -> Result<()> {
    let (policy, _) = config::load_policy(workspace, None, true)?;
    println!(
        "Secret backend: {}",
        if std::env::var("SAFE_AGENT_TEST_SECRET_BACKEND")
            .ok()
            .as_deref()
            == Some("memory")
        {
            "test memory"
        } else {
            "macOS Keychain"
        }
    );
    if policy.secrets.is_empty() {
        println!("No project secrets declared.");
        return Ok(());
    }
    let mappings = fs::read_to_string(config::secrets_path()).unwrap_or_default();
    for name in policy.secrets.keys() {
        println!(
            "{}: {}",
            name,
            if mappings.contains(name) {
                "mapping configured"
            } else {
                "missing mapping"
            }
        );
    }
    Ok(())
}

pub fn get(name: &str, project: &Path) -> Result<String> {
    if std::env::var("SAFE_AGENT_TEST_SECRET_BACKEND")
        .ok()
        .as_deref()
        == Some("memory")
    {
        let content = fs::read_to_string(config::secrets_path().with_extension("values"))?;
        return content
            .lines()
            .find_map(|line| line.strip_prefix(&format!("{name}=")).map(str::to_owned))
            .ok_or_else(|| anyhow::anyhow!("secret {name} not found"));
    }
    let service = format!("safe-agent/{}", project.display());
    let output = Command::new("security")
        .args(["find-generic-password", "-a", name, "-s", &service, "-w"])
        .output()?;
    if !output.status.success() {
        bail!("secret {name} not found in Keychain");
    }
    Ok(String::from_utf8(output.stdout)?.trim_end().to_owned())
}
