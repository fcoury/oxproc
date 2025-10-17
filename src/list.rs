use crate::config::{self, ConfigSource, TaskKind};
use crate::task;
use anyhow::Result;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct TaskInfo {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub parallel: bool,
}

#[derive(Debug, Serialize)]
pub struct ListInfo {
    pub source: ConfigSource,
    pub processes: Vec<String>,
    pub tasks: Vec<TaskInfo>,
}

pub fn gather_list_info(root: &Path) -> Result<ListInfo> {
    let source = config::detect_source(root)?;
    let mut processes = config::load_config_from(root)?
        .into_iter()
        .map(|p| p.name)
        .collect::<Vec<_>>();
    processes.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));

    let mut tasks: Vec<TaskInfo> = Vec::new();
    if let Some(map) = config::load_tasks_from(root)? {
        let mut items: Vec<(String, TaskInfo)> = Vec::new();
        for (k, v) in map.iter() {
            let name_display = task::display_task_name(k);
            let info = match &v.kind {
                TaskKind::Shell { .. } => TaskInfo {
                    name: name_display,
                    kind: "shell".to_string(),
                    children: Vec::new(),
                    parallel: false,
                },
                TaskKind::Composite { children, parallel } => {
                    // Resolve children relative to the current task for display
                    let mut resolved: Vec<String> = children
                        .iter()
                        .map(|c| task::display_task_name(&task::resolve_child_name(k, c)))
                        .collect();
                    resolved.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
                    TaskInfo {
                        name: name_display,
                        kind: "composite".to_string(),
                        children: resolved,
                        parallel: *parallel,
                    }
                }
            };
            items.push((k.clone(), info));
        }
        items.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
        tasks = items.into_iter().map(|(_, i)| i).collect();
    }

    Ok(ListInfo {
        source,
        processes,
        tasks,
    })
}

pub fn format_list_human(
    info: &ListInfo,
    processes_only: bool,
    tasks_only: bool,
) -> String {
    let mut out = String::new();
    use std::fmt::Write as _;
    let _ = writeln!(out, "Source: {:?}", info.source);

    let show_processes = !tasks_only;
    let show_tasks = !processes_only;

    if show_processes {
        let _ = writeln!(out, "Processes ({}):", info.processes.len());
        if info.processes.is_empty() {
            let _ = writeln!(out, "  (none)");
        } else {
            for p in &info.processes {
                let _ = writeln!(out, "  {}", p);
            }
        }
    }

    if show_tasks {
        match info.source {
            ConfigSource::Procfile => {
                let _ = writeln!(out, "Tasks: (not available with Procfile)");
            }
            ConfigSource::ProcToml => {
                let _ = writeln!(out, "Tasks ({}):", info.tasks.len());
                if info.tasks.is_empty() {
                    let _ = writeln!(out, "  (none)");
                } else {
                    for t in &info.tasks {
                        match t.kind.as_str() {
                            "composite" => {
                                if t.children.is_empty() {
                                    let _ = writeln!(out, "  {} (group)", t.name);
                                } else {
                                    let _ = writeln!(out, "  {} (group: {})", t.name, t.children.join(", "));
                                }
                            }
                            _ => {
                                let _ = writeln!(out, "  {}", t.name);
                            }
                        }
                    }
                }
            }
        }
    }

    out
}

pub fn format_list_names_only(
    info: &ListInfo,
    processes_only: bool,
    tasks_only: bool,
) -> String {
    let show_processes = !tasks_only;
    let show_tasks = !processes_only;
    let mut lines: Vec<String> = Vec::new();
    if show_processes {
        lines.extend(info.processes.clone());
    }
    if show_tasks && matches!(info.source, ConfigSource::ProcToml) {
        lines.extend(info.tasks.iter().map(|t| t.name.clone()));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn gathers_processes_and_tasks_from_proc_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("proc.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(
            f,
            r#"[web]
cmd = "echo web"

[tasks.frontend.build]
cmd = "echo build""#
        )
        .unwrap();

        let info = gather_list_info(dir.path()).unwrap();
        assert_eq!(info.processes, vec!["web".to_string()]);
        assert_eq!(info.tasks.len(), 1);
        assert_eq!(info.tasks[0].name, "frontend:build");
        let human = format_list_human(&info, false, false);
        assert!(human.contains("Processes (1):"));
        assert!(human.contains("Tasks (1):"));
    }

    #[test]
    fn gathers_from_procfile_without_tasks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Procfile");
        std::fs::write(&path, "web: echo web\nworker: echo worker\n").unwrap();
        let info = gather_list_info(dir.path()).unwrap();
        assert_eq!(info.processes.len(), 2);
        assert!(info.tasks.is_empty());
        let human = format_list_human(&info, false, false);
        assert!(human.contains("Tasks: (not available with Procfile)"));
    }
}
