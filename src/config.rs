use std::collections::HashMap;
use std::fs;
use std::path::Path;
use thiserror::Error;

// Flexible TOML layout support:
// - Processes can live under [processes.<name>] or as top-level tables (legacy)
// - Tasks live under [tasks.<name>]
// We parse via toml::Value to support both forms simultaneously.

// Note: we no longer use a fixed struct for process entries because we
// support both [processes.<name>] and top-level tables; we parse via
// toml::Value for flexibility.

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
    pub cmd: String,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigSource {
    ProcToml,
    Procfile,
}

pub fn detect_source(root: &Path) -> Result<ConfigSource, ConfigError> {
    let proc_toml = root.join("proc.toml");
    let procfile = root.join("Procfile");
    if proc_toml.exists() {
        Ok(ConfigSource::ProcToml)
    } else if procfile.exists() {
        Ok(ConfigSource::Procfile)
    } else {
        Err(ConfigError::NoConfigFile)
    }
}

pub fn load_config_from(root: &Path) -> Result<Vec<ProcessConfig>, ConfigError> {
    match detect_source(root)? {
        ConfigSource::ProcToml => load_processes_from_toml(&root.join("proc.toml")),
        ConfigSource::Procfile => load_processes_from_procfile(&root.join("Procfile")),
    }
}

fn load_processes_from_procfile(path: &Path) -> Result<Vec<ProcessConfig>, ConfigError> {
    let content = fs::read_to_string(path)?;
    if content.trim().is_empty() {
        return Err(ConfigError::EmptyProcfile);
    }
    let mut configs = Vec::new();
    for line in content.lines() {
        if let Some((name, command)) = line.split_once(':') {
            configs.push(ProcessConfig {
                name: name.trim().to_string(),
                command: command.trim().to_string(),
                stdout_log: None,
                stderr_log: None,
                cwd: None,
            });
        }
    }
    Ok(configs)
}

fn load_processes_from_toml(path: &Path) -> Result<Vec<ProcessConfig>, ConfigError> {
    let content = fs::read_to_string(path)?;
    let value: toml::Value = toml::from_str(&content)?;

    let mut processes: HashMap<String, ProcessConfig> = HashMap::new();

    // 1) Explicit [processes.<name>]
    if let Some(proc_tbl) = value.get("processes").and_then(|v| v.as_table()) {
        for (name, item) in proc_tbl.iter() {
            if let Some(tbl) = item.as_table() {
                if let Some(cmd) = tbl.get("cmd").and_then(|v| v.as_str()) {
                    let stdout = tbl.get("stdout").and_then(|v| v.as_str()).map(|s| s.to_string());
                    let stderr = tbl.get("stderr").and_then(|v| v.as_str()).map(|s| s.to_string());
                    let cwd = tbl.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
                    processes.insert(
                        name.clone(),
                        ProcessConfig {
                            name: name.clone(),
                            command: cmd.to_string(),
                            stdout_log: stdout,
                            stderr_log: stderr,
                            cwd,
                        },
                    );
                }
            }
        }
    }

    // 2) Top-level tables (back-compat). Skip reserved key "tasks".
    if let Some(root_tbl) = value.as_table() {
        for (name, item) in root_tbl.iter() {
            if name == "tasks" || name == "processes" {
                continue;
            }
            if processes.contains_key(name) {
                continue; // Prefer explicit [processes]
            }
            if let Some(tbl) = item.as_table() {
                if let Some(cmd) = tbl.get("cmd").and_then(|v| v.as_str()) {
                    let stdout = tbl.get("stdout").and_then(|v| v.as_str()).map(|s| s.to_string());
                    let stderr = tbl.get("stderr").and_then(|v| v.as_str()).map(|s| s.to_string());
                    let cwd = tbl.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
                    processes.insert(
                        name.clone(),
                        ProcessConfig {
                            name: name.clone(),
                            command: cmd.to_string(),
                            stdout_log: stdout,
                            stderr_log: stderr,
                            cwd,
                        },
                    );
                }
            }
        }
    }

    Ok(processes.into_values().collect())
}

pub fn load_tasks_from(root: &Path) -> Result<Option<HashMap<String, TaskConfig>>, ConfigError> {
    match detect_source(root)? {
        ConfigSource::Procfile => Ok(None),
        ConfigSource::ProcToml => {
            let content = fs::read_to_string(root.join("proc.toml"))?;
            let value: toml::Value = toml::from_str(&content)?;
            let mut tasks: HashMap<String, TaskConfig> = HashMap::new();
            if let Some(tbl) = value.get("tasks").and_then(|v| v.as_table()) {
                fn collect_tasks(prefix: &str, table: &toml::value::Table, tasks: &mut HashMap<String, TaskConfig>) {
                    for (key, val) in table.iter() {
                        if let Some(child) = val.as_table() {
                            let full = if prefix.is_empty() {
                                key.clone()
                            } else {
                                format!("{}.{}", prefix, key)
                            };

                            if let Some(cmd) = child.get("cmd").and_then(|v| v.as_str()) {
                                let cwd = child.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
                                tasks.insert(
                                    full.clone(),
                                    TaskConfig {
                                        name: full.clone(),
                                        cmd: cmd.to_string(),
                                        cwd,
                                    },
                                );
                            }

                            // Recurse to allow dotted namespaces: [tasks.frontend.build]
                            collect_tasks(&full, child, tasks);
                        }
                    }
                }

                collect_tasks("", tbl, &mut tasks);
            }
            Ok(Some(tasks))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn loads_processes_from_top_level_and_tasks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("proc.toml");
        let mut file = std::fs::File::create(&path).unwrap();
        writeln!(
            file,
            r#"
[web]
cmd = "echo web"

[tasks.build]
cmd = "echo build"
"#
        )
        .unwrap();

        let procs = load_processes_from_toml(&path).unwrap();
        assert_eq!(procs.len(), 1);
        assert_eq!(procs[0].name, "web");

        let tasks = load_tasks_from(dir.path()).unwrap().unwrap();
        assert!(tasks.contains_key("build"));
        assert_eq!(tasks.get("build").unwrap().cmd, "echo build");
    }

    #[test]
    fn loads_processes_from_processes_table() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("proc.toml");
        std::fs::write(
            &path,
            r#"
[processes.web]
cmd = "echo web"
[processes.worker]
cmd = "echo worker"
"#,
        )
        .unwrap();

        let mut procs = load_processes_from_toml(&path).unwrap();
        procs.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(procs.len(), 2);
        assert_eq!(procs[0].name, "web");
        assert_eq!(procs[1].name, "worker");
    }

    #[test]
    fn tasks_absent_returns_empty_map() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("proc.toml");
        std::fs::write(
            &path,
            r#"
[web]
cmd = "echo web"
"#,
        )
        .unwrap();

        let tasks = load_tasks_from(dir.path()).unwrap().unwrap();
        assert!(tasks.is_empty());
    }

    #[test]
    fn loads_nested_tasks_with_dots() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("proc.toml");
        std::fs::write(
            &path,
            r#"
[tasks.frontend.build]
cmd = "pnpm run build"
cwd = "./frontend"

[tasks.api.migrate]
cmd = "cargo run --bin api -- migrate"
"#,
        )
        .unwrap();

        let tasks = load_tasks_from(dir.path()).unwrap().unwrap();
        assert!(tasks.contains_key("frontend.build"));
        assert!(tasks.contains_key("api.migrate"));
        assert_eq!(tasks.get("frontend.build").unwrap().cwd.as_deref(), Some("./frontend"));
    }
}
