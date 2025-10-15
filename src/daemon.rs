#[cfg(unix)]
use crate::{config::load_config_from, dirs, manager, state};
#[cfg(unix)]
use anyhow::Result;
#[cfg(unix)]
use daemonize::Daemonize;
#[cfg(unix)]
use fs2::FileExt;
#[cfg(unix)]
use std::fs::{self, OpenOptions};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
// no path buffer needed here
#[cfg(unix)]
use tokio::runtime::Builder;

#[cfg(unix)]
pub fn start_daemon(root: &std::path::Path) -> Result<()> {
    // Resolve state dir and create it
    let project_root = dirs::normalize_root(root)?;
    let state_dir = dirs::state_dir_for_project(&project_root);
    fs::create_dir_all(&state_dir)?;

    // Clean up stale pid file if present
    let _ = state::cleanup_stale_state_if_any(&project_root);

    // Acquire a simple lock to avoid concurrent daemons
    let lock_path = state::manager_lock_path(&state_dir);
    let lock_file = OpenOptions::new()
        .create(true)
        .write(true)
        .mode(0o600)
        .open(&lock_path)?;
    lock_file.try_lock_exclusive().map_err(|_| {
        anyhow::anyhow!(
            "Another oxproc daemon seems to be running (lock held at {}).",
            lock_path.display()
        )
    })?;

    let manager_log = state::manager_log_path(&state_dir);
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(&manager_log)?;

    let pid_path = state::manager_pid_path(&state_dir);

    let daemonize = Daemonize::new()
        .pid_file(&pid_path)
        .chown_pid_file(true)
        .working_directory(&project_root)
        .stdout(log_file.try_clone()?)
        .stderr(log_file.try_clone()?);

    match daemonize.start() {
        Ok(()) => {
            // We are in the daemon process now
            let rt = Builder::new_multi_thread().enable_all().build()?;
            rt.block_on(async move {
                let configs = load_config_from(&project_root)?;
                manager::run_manager_daemon(configs, state_dir, &project_root).await
            })?
        }
        Err(e) => {
            return Err(anyhow::anyhow!("Failed to daemonize: {}", e));
        }
    }

    Ok(())
}
