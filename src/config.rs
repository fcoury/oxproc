use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Deserialize)]
pub struct TomlConfig {
    #[serde(flatten)]
    pub processes: HashMap<String, ProcessDetails>,
    #[serde(default)]
    pub tasks: HashMap<String, TaskDefinition>,
}

#[derive(Debug, Deserialize)]
pub struct ProcessDetails {
    pub cmd: String,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub cwd: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum TaskDefinition {
    Command(String),
    Detailed(TaskDetails),
}

#[derive(Debug, Deserialize)]
pub struct TaskDetails {
    pub cmd: String,
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
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigKind {
    ProcToml,
    Procfile,
}

#[derive(Debug, Clone)]
pub struct ProjectConfig {
    pub kind: ConfigKind,
    pub processes: Vec<ProcessConfig>,
    pub tasks: Vec<TaskConfig>,
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
}

pub fn load_project_config(root: &Path) -> Result<ProjectConfig, ConfigError> {
    let proc_toml = root.join("proc.toml");
    let procfile = root.join("Procfile");
    if proc_toml.exists() {
        let content = fs::read_to_string(proc_toml)?;
        let toml_config: TomlConfig = toml::from_str(&content)?;
        let mut processes = Vec::new();
        for (name, details) in toml_config.processes {
            processes.push(ProcessConfig {
                name,
                command: details.cmd,
                stdout_log: details.stdout,
                stderr_log: details.stderr,
                cwd: details.cwd,
            });
        }
        let mut tasks = Vec::new();
        for (name, details) in toml_config.tasks {
            match details {
                TaskDefinition::Command(cmd) => tasks.push(TaskConfig {
                    name,
                    command: cmd,
                    cwd: None,
                }),
                TaskDefinition::Detailed(task) => tasks.push(TaskConfig {
                    name,
                    command: task.cmd,
                    cwd: task.cwd,
                }),
            }
        }
        Ok(ProjectConfig {
            kind: ConfigKind::ProcToml,
            processes,
            tasks,
        })
    } else if procfile.exists() {
        let content = fs::read_to_string(procfile)?;
        if content.trim().is_empty() {
            return Err(ConfigError::EmptyProcfile);
        }
        let mut processes = Vec::new();
        for line in content.lines() {
            if let Some((name, command)) = line.split_once(':') {
                processes.push(ProcessConfig {
                    name: name.trim().to_string(),
                    command: command.trim().to_string(),
                    stdout_log: None,
                    stderr_log: None,
                    cwd: None,
                });
            }
        }
        Ok(ProjectConfig {
            kind: ConfigKind::Procfile,
            processes,
            tasks: Vec::new(),
        })
    } else {
        Err(ConfigError::NoConfigFile)
    }
}

pub fn load_process_configs(root: &Path) -> Result<Vec<ProcessConfig>, ConfigError> {
    Ok(load_project_config(root)?.processes)
}
