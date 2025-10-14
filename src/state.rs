use crate::dirs::state_dir_for_project;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ManagerInfo {
    pub pid: u32,
    pub started_at: DateTime<Utc>,
    pub project_root: String,
    pub version: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProcessInfo {
    pub name: String,
    pub pid: u32,
    pub pgid: i32,
    pub cmd: String,
    pub cwd: Option<String>,
    pub stdout_log: String,
    pub stderr_log: String,
    pub started_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ManagerState {
    pub manager: ManagerInfo,
    pub processes: Vec<ProcessInfo>,
}

pub fn state_dir_from_root(root: &Path) -> PathBuf { state_dir_for_project(root) }

pub fn state_file_path(dir: &Path) -> PathBuf {
    dir.join("state.json")
}

pub fn manager_pid_path(dir: &Path) -> PathBuf {
    dir.join("manager.pid")
}

pub fn manager_lock_path(dir: &Path) -> PathBuf {
    dir.join("manager.lock")
}

pub fn manager_log_path(dir: &Path) -> PathBuf {
    dir.join("manager.log")
}

pub fn save_state(dir: &Path, state: &ManagerState) -> anyhow::Result<()> {
    fs::create_dir_all(dir)?;
    let tmp = dir.join("state.json.tmp");
    let mut f = fs::File::create(&tmp)?;
    serde_json::to_writer_pretty(&mut f, state)?;
    f.flush()?;
    fs::rename(tmp, state_file_path(dir))?;
    Ok(())
}

pub fn load_state_from_root(root: &Path) -> anyhow::Result<ManagerState> {
    let dir = state_dir_from_root(root);
    let data = fs::read_to_string(state_file_path(&dir))?;
    let st: ManagerState = serde_json::from_str(&data)?;
    Ok(st)
}

pub fn print_status(root: &Path) -> anyhow::Result<()> {
    use nix::sys::signal::kill;
    use nix::unistd::Pid;

    let st = match load_state_from_root(root) {
        Ok(s) => s,
        Err(_) => {
            println!("No daemon state found for this project.");
            return Ok(());
        }
    };
    println!("Manager PID: {} (since {})", st.manager.pid, st.manager.started_at);
    println!("Processes:");
    for p in &st.processes {
        let alive = kill(Pid::from_raw(p.pid as i32), None).is_ok();
        println!(
            "- {:<12} pid={} pgid={} alive={} cmd={}",
            p.name, p.pid, p.pgid, alive, p.cmd
        );
    }
    Ok(())
}

pub fn cleanup_stale_state_if_any(root: &Path) -> anyhow::Result<bool> {
    use nix::sys::signal::kill;
    use nix::unistd::Pid;

    let dir = state_dir_from_root(root);
    let pid_path = manager_pid_path(&dir);
    if !pid_path.exists() {
        return Ok(false);
    }
    let pid_str = match fs::read_to_string(&pid_path) {
        Ok(s) => s,
        Err(_) => return Ok(false),
    };
    let pid: i32 = match pid_str.trim().parse() {
        Ok(p) => p,
        Err(_) => return Ok(false),
    };
    let alive = kill(Pid::from_raw(pid), None).is_ok();
    if !alive {
        let _ = fs::remove_file(&pid_path);
        // state.json may still be useful, keep it
        println!("Removed stale manager.pid (pid {}).", pid);
        return Ok(true);
    }
    Ok(false)
}
