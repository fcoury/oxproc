use crate::dirs::state_dir_for_project;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

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

pub fn state_dir_from_root(root: &Path) -> PathBuf {
    state_dir_for_project(root)
}

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
    println!(
        "Manager PID: {} (since {})",
        st.manager.pid, st.manager.started_at
    );
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

pub fn wait_for_manager_ready(root: &Path, timeout: Duration) -> anyhow::Result<()> {
    use std::time::{Duration as StdDuration, Instant};

    let dir = state_dir_from_root(root);
    let state_path = state_file_path(&dir);
    let start = Instant::now();
    let mut last_err: Option<anyhow::Error> = None;

    while start.elapsed() < timeout {
        match fs::read_to_string(&state_path) {
            Ok(data) => {
                if let Ok(st) = serde_json::from_str::<ManagerState>(&data) {
                    // Consider ready if file is valid; processes list can be empty in edge cases
                    if !st.manager.project_root.is_empty() {
                        return Ok(());
                    }
                }
            }
            Err(e) => {
                last_err = Some(anyhow::anyhow!(e));
            }
        }
        std::thread::sleep(StdDuration::from_millis(200));
    }

    Err(anyhow::anyhow!(
        "Timed out waiting for manager state at {} (last error: {:?})",
        state_path.display(),
        last_err
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn unique_temp_dir(name: &str) -> PathBuf {
        let mut p = env::temp_dir();
        let nonce = format!("oxproc-test-{}-{}", name, std::process::id());
        p.push(nonce);
        let _ = std::fs::create_dir_all(&p);
        p
    }

    #[test]
    fn wait_for_manager_ready_times_out_when_absent() {
        let root = unique_temp_dir("root-timeout");
        let state_home = unique_temp_dir("state-timeout");
        env::set_var("XDG_STATE_HOME", &state_home);
        let res = wait_for_manager_ready(&root, Duration::from_millis(700));
        assert!(res.is_err());
    }

    #[test]
    fn wait_for_manager_ready_succeeds_when_state_present() {
        let root = unique_temp_dir("root-ready");
        let state_home = unique_temp_dir("state-ready");
        env::set_var("XDG_STATE_HOME", &state_home);

        // Prepare a minimal valid state.json
        let dir = state_dir_from_root(&root);
        let _ = std::fs::create_dir_all(&dir);
        let st = ManagerState {
            manager: ManagerInfo {
                pid: 12345,
                started_at: Utc::now(),
                project_root: root.to_string_lossy().to_string(),
                version: 1,
            },
            processes: vec![],
        };
        save_state(&dir, &st).expect("write state");

        let res = wait_for_manager_ready(&root, Duration::from_secs(1));
        assert!(res.is_ok());
    }
}
