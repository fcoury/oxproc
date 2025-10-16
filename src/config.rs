use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Deserialize)]
struct ProcTomlConfig {
    #[serde(default)]
    processes: HashMap<String, ProcessDetails>,
    #[serde(default)]
    tasks: HashMap<String, ProcessDetails>,
    #[serde(default, flatten)]
    legacy_processes: HashMap<String, ProcessDetails>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ProcessDetails {
    pub cmd: String,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ProcessConfig {
    pub name: String,
    pub command: String,
    pub stdout_log: Option<String>,
    pub stderr_log: Option<String>,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TaskConfig {
    pub name: String,
    pub command: String,
    pub stdout_log: Option<String>,
    pub stderr_log: Option<String>,
    pub cwd: Option<String>,
}

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Neither proc.toml nor Procfile found in the current directory")]
    NoConfigFile,
    #[error("Failed to read file: {0}")]
    FileReadError(#[from] std::io::Error),
    #[error("Failed to parse proc.toml: {0}")]
    TomlParseError(#[from] toml::de::Error),
    #[error("Procfile is empty")]
    EmptyProcfile,
    #[error("Tasks require proc.toml; Procfile does not support task definitions")]
    TasksUnavailableFromProcfile,
}

pub fn load_config_from(root: &Path) -> Result<Vec<ProcessConfig>, ConfigError> {
    load_process_configs(root)
}

pub fn load_process_configs(root: &Path) -> Result<Vec<ProcessConfig>, ConfigError> {
    if let Some(config) = read_proc_toml(root)? {
        return Ok(config.into_process_configs());
    }
    load_process_configs_from_procfile(root)
}

pub fn load_task_configs(root: &Path) -> Result<Vec<TaskConfig>, ConfigError> {
    match read_proc_toml(root)? {
        Some(config) => Ok(config.into_task_configs()),
        None => {
            if root.join("Procfile").exists() {
                Err(ConfigError::TasksUnavailableFromProcfile)
            } else {
                Err(ConfigError::NoConfigFile)
            }
        }
    }
}

fn read_proc_toml(root: &Path) -> Result<Option<ProcTomlConfig>, ConfigError> {
    let proc_toml = root.join("proc.toml");
    if !proc_toml.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(proc_toml)?;
    let config: ProcTomlConfig = toml::from_str(&content)?;
    Ok(Some(config))
}

fn load_process_configs_from_procfile(root: &Path) -> Result<Vec<ProcessConfig>, ConfigError> {
    let procfile = root.join("Procfile");
    if !procfile.exists() {
        return Err(ConfigError::NoConfigFile);
    }
    let content = fs::read_to_string(procfile)?;
    if content.trim().is_empty() {
        return Err(ConfigError::EmptyProcfile);
    }
    let mut configs = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((name, command)) = trimmed.split_once(':') {
            let name = name.trim();
            let command = command.trim();
            configs.push(ProcessConfig {
                name: name.to_string(),
                command: command.to_string(),
                stdout_log: None,
                stderr_log: None,
                cwd: None,
            });
        }
    }
    Ok(configs)
}

impl ProcTomlConfig {
    fn into_process_configs(self) -> Vec<ProcessConfig> {
        let ProcTomlConfig {
            processes,
            tasks: _,
            legacy_processes,
        } = self;
        let mut merged: BTreeMap<String, ProcessDetails> =
            legacy_processes.into_iter().collect();
        for (name, details) in processes {
            merged.insert(name, details);
        }
        merged
            .into_iter()
            .map(|(name, details)| ProcessConfig::from_details(name, details))
            .collect()
    }

    fn into_task_configs(self) -> Vec<TaskConfig> {
        let ProcTomlConfig {
            processes: _,
            tasks,
            legacy_processes: _,
        } = self;
        tasks
            .into_iter()
            .collect::<BTreeMap<String, ProcessDetails>>()
            .into_iter()
            .map(|(name, details)| TaskConfig::from_details(name, details))
            .collect()
    }
}

impl ProcessConfig {
    fn from_details(name: String, details: ProcessDetails) -> Self {
        ProcessConfig {
            name,
            command: details.cmd,
            stdout_log: details.stdout,
            stderr_log: details.stderr,
            cwd: details.cwd,
        }
    }
}

impl TaskConfig {
    fn from_details(name: String, details: ProcessDetails) -> Self {
        TaskConfig {
            name,
            command: details.cmd,
            stdout_log: details.stdout,
            stderr_log: details.stderr,
            cwd: details.cwd,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn unique_temp_dir(name: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        let nonce = format!(
            "oxproc-config-test-{}-{}-{}",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        dir.push(nonce);
        let _ = fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn load_task_configs_parses_tasks_section() {
        let root = unique_temp_dir("tasks-section");
        let toml = r#"
[processes]
web = { cmd = "bundle exec rails server", stdout = "log/web.out" }

[tasks]
migrate = { cmd = "bin/rails db:migrate", cwd = "services/api" }
cache = { cmd = "bin/rake cache:prime", stdout = "log/cache.out" }
"#;
        fs::write(root.join("proc.toml"), toml).unwrap();

        let tasks = load_task_configs(&root).unwrap();
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].name, "cache");
        assert_eq!(tasks[0].command, "bin/rake cache:prime");
        assert_eq!(tasks[0].stdout_log.as_deref(), Some("log/cache.out"));
        assert_eq!(tasks[1].name, "migrate");
        assert_eq!(tasks[1].cwd.as_deref(), Some("services/api"));

        let processes = load_process_configs(&root).unwrap();
        assert_eq!(processes.len(), 1);
        assert_eq!(processes[0].name, "web");
        assert_eq!(
            processes[0].command,
            "bundle exec rails server"
        );
        assert_eq!(
            processes[0].stdout_log.as_deref(),
            Some("log/web.out")
        );
    }

    #[test]
    fn load_process_configs_supports_legacy_top_level_tables() {
        let root = unique_temp_dir("legacy-layout");
        let toml = r#"
[worker]
cmd = "python worker.py"
stderr = "log/worker.err"

[web]
cmd = "npm start"
"#;
        fs::write(root.join("proc.toml"), toml).unwrap();

        let processes = load_process_configs(&root).unwrap();
        let names: Vec<_> = processes.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["web", "worker"]);
        let worker = processes.iter().find(|p| p.name == "worker").unwrap();
        assert_eq!(worker.stderr_log.as_deref(), Some("log/worker.err"));
    }

    #[test]
    fn load_task_configs_errors_when_only_procfile_present() {
        let root = unique_temp_dir("procfile-only");
        fs::write(root.join("Procfile"), "web: npm start\n").unwrap();

        let err = load_task_configs(&root).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::TasksUnavailableFromProcfile
        ));
    }
}
