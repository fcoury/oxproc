use crate::config::ProcessConfig;
use crate::state::{load_state_from_root, save_state, ManagerInfo, ManagerState, ProcessInfo};
use anyhow::Result;
use chrono::Utc;
use futures::future::join_all;
use std::process::Stdio;
use std::sync::Arc;
use tokio::fs::OpenOptions;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::Mutex;

#[cfg(unix)]
use nix::sys::signal::{kill, Signal};
#[cfg(unix)]
use nix::unistd::{getpgid, setsid, Pid};

pub async fn run_manager_daemon(
    configs: Vec<ProcessConfig>,
    state_dir: std::path::PathBuf,
    root: &std::path::Path,
) -> Result<()> {
    let mut children = Vec::new();
    let mut handles = Vec::new();
    let mut proc_infos: Vec<ProcessInfo> = Vec::new();

    for config in configs {
        let mut cmd = Command::new("sh");
        cmd.arg("-c");
        cmd.arg(&config.command);
        if let Some(cwd) = &config.cwd {
            let abs = if std::path::Path::new(cwd).is_absolute() {
                std::path::PathBuf::from(cwd)
            } else {
                root.join(cwd)
            };
            if !abs.exists() {
                return Err(anyhow::anyhow!(
                    "Process '{}' cwd does not exist: {}",
                    config.name,
                    abs.display()
                ));
            }
            cmd.current_dir(abs);
        }
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        // Each child gets its own session/PGID
        unsafe {
            cmd.pre_exec(|| {
                // SAFETY: called in child just before exec
                match setsid() {
                    Ok(_) => Ok(()),
                    Err(e) => Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("setsid failed: {}", e),
                    )),
                }
            });
        }

        let mut child = cmd.spawn()?;
        let pid = child.id().unwrap();
        let pgid = getpgid(Some(Pid::from_raw(pid as i32)))
            .unwrap_or(Pid::from_raw(pid as i32))
            .as_raw();

        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        let stdout_log = config
            .stdout_log
            .clone()
            .unwrap_or_else(|| format!("{}.out.log", config.name));
        let stderr_log = config
            .stderr_log
            .clone()
            .unwrap_or_else(|| format!("{}.err.log", config.name));

        let out_handle = tokio::spawn(handle_output(
            config.name.clone(),
            stdout,
            Some(if std::path::Path::new(&stdout_log).is_absolute() {
                stdout_log.clone()
            } else {
                root.join(&stdout_log).to_string_lossy().to_string()
            }),
            false,
            "",
        ));
        let err_handle = tokio::spawn(handle_output(
            config.name.clone(),
            stderr,
            Some(if std::path::Path::new(&stderr_log).is_absolute() {
                stderr_log.clone()
            } else {
                root.join(&stderr_log).to_string_lossy().to_string()
            }),
            false,
            "[ERR] ",
        ));

        handles.push(out_handle);
        handles.push(err_handle);

        proc_infos.push(ProcessInfo {
            name: config.name.clone(),
            pid,
            pgid,
            cmd: config.command.clone(),
            cwd: config.cwd.clone(),
            stdout_log,
            stderr_log,
            started_at: Utc::now(),
        });

        children.push(Arc::new(Mutex::new(child)));
    }

    let state = ManagerState {
        manager: ManagerInfo {
            pid: std::process::id(),
            started_at: Utc::now(),
            project_root: root.to_string_lossy().to_string(),
            version: 1,
        },
        processes: proc_infos,
    };
    save_state(&state_dir, &state)?;

    // Wait on either child completion or termination signal
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;

    tokio::select! {
        _ = join_all(handles) => {
            // One of the streams finished; keep running until terminated, but we'll just park here
            sigterm.recv().await;
        }
        _ = sigterm.recv() => {}
        _ = sigint.recv() => {}
    }

    // Graceful shutdown: SIGTERM to each process group, then SIGKILL after 5s
    for child in &children {
        let c = child.lock().await;
        if let Some(pid) = c.id() {
            let pgid =
                getpgid(Some(Pid::from_raw(pid as i32))).unwrap_or(Pid::from_raw(pid as i32));
            let _ = kill(Pid::from_raw(-pgid.as_raw()), Signal::SIGTERM);
        }
    }
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    for child in &children {
        let c = child.lock().await;
        if let Some(pid) = c.id() {
            let pgid =
                getpgid(Some(Pid::from_raw(pid as i32))).unwrap_or(Pid::from_raw(pid as i32));
            let _ = kill(Pid::from_raw(-pgid.as_raw()), Signal::SIGKILL);
        }
    }

    Ok(())
}

async fn handle_output<T: AsyncRead + Unpin>(
    child_name: String,
    stream: T,
    log_path: Option<String>,
    follow: bool,
    prefix: &'static str,
) {
    let mut reader = BufReader::new(stream).lines();
    let mut file = if let Some(path) = log_path {
        if let Some(parent) = std::path::Path::new(&path).parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        Some(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .await
                .unwrap(),
        )
    } else {
        None
    };

    while let Some(line) = reader.next_line().await.unwrap() {
        if follow {
            println!("[{}] {}{}", child_name, prefix, line);
        } else if let Some(ref mut file) = file {
            file.write_all(format!("{}\n", line).as_bytes())
                .await
                .unwrap();
        }
    }
}

#[cfg(unix)]
pub fn stop_all(root: &std::path::Path, grace: Option<std::time::Duration>) -> Result<()> {
    let st = match load_state_from_root(root) {
        Ok(s) => s,
        Err(_) => {
            println!("No daemon state found for this project.");
            return Ok(());
        }
    };
    let grace = grace.unwrap_or(std::time::Duration::from_secs(5));

    println!(
        "Stopping {} process(es) (manager PID {})...",
        st.processes.len(),
        st.manager.pid
    );

    // Send SIGTERM to each process group
    for p in &st.processes {
        match kill(nix::unistd::Pid::from_raw(-p.pgid), Signal::SIGTERM) {
            Ok(_) => println!(
                "- sent SIGTERM to {} (pid {}, pgid {})",
                p.name, p.pid, p.pgid
            ),
            Err(e) => println!("- {} already stopped or cannot signal ({}).", p.name, e),
        }
    }
    println!("Waiting {}s for graceful shutdown...", grace.as_secs());
    std::thread::sleep(grace);

    // Escalate with SIGKILL where needed
    let mut killed = 0usize;
    for p in &st.processes {
        let alive = kill(nix::unistd::Pid::from_raw(p.pid as i32), None).is_ok();
        if alive {
            let _ = kill(nix::unistd::Pid::from_raw(-p.pgid), Signal::SIGKILL);
            println!("- escalated SIGKILL to {} (pgid {})", p.name, p.pgid);
            killed += 1;
        }
    }

    // Terminate manager last
    println!("Stopping manager (pid {})...", st.manager.pid);
    let _ = kill(
        nix::unistd::Pid::from_raw(st.manager.pid as i32),
        Signal::SIGTERM,
    );
    std::thread::sleep(std::time::Duration::from_millis(300));
    if kill(nix::unistd::Pid::from_raw(st.manager.pid as i32), None).is_ok() {
        let _ = kill(
            nix::unistd::Pid::from_raw(st.manager.pid as i32),
            Signal::SIGKILL,
        );
    }

    // Attempt to clean up pid/lock files for this project
    use std::fs;
    let dir = crate::state::state_dir_from_root(root);
    let pid_path = crate::state::manager_pid_path(&dir);
    let lock_path = crate::state::manager_lock_path(&dir);
    let mut removed = Vec::new();
    if pid_path.exists() {
        if fs::remove_file(&pid_path).is_ok() {
            removed.push("manager.pid");
        }
    }
    if lock_path.exists() {
        if fs::remove_file(&lock_path).is_ok() {
            removed.push("manager.lock");
        }
    }

    println!("Stop complete. {} process(es) required SIGKILL.", killed);
    if !removed.is_empty() {
        println!(
            "State cleaned up at {} (removed: {}).",
            dir.display(),
            removed.join(", ")
        );
    }
    Ok(())
}

#[cfg(not(unix))]
pub fn stop_all(_grace: Option<std::time::Duration>) -> Result<()> {
    anyhow::bail!("Stop is only supported on Unix in daemon mode")
}

pub fn print_logs(
    root: &std::path::Path,
    name: Option<String>,
    follow: bool,
    _lines: usize,
) -> Result<()> {
    let st = match load_state_from_root(root) {
        Ok(s) => s,
        Err(_) => {
            println!("No daemon state found for this project.");
            return Ok(());
        }
    };
    let selected: Vec<_> = st
        .processes
        .iter()
        .filter(|p| name.as_ref().map(|n| n == &p.name).unwrap_or(true))
        .cloned()
        .collect();

    if selected.is_empty() {
        println!("No matching processes.");
        return Ok(());
    }

    if follow {
        follow_combined(selected, _lines, root)?;
    } else {
        print_tail(selected, _lines, root)?;
    }
    Ok(())
}

fn resolve_path(root: &std::path::Path, p: &str) -> String {
    if std::path::Path::new(p).is_absolute() {
        p.to_string()
    } else {
        root.join(p).to_string_lossy().to_string()
    }
}

fn print_tail(processes: Vec<ProcessInfo>, lines: usize, root: &std::path::Path) -> Result<()> {
    for p in processes {
        println!("== {} ==", p.name);
        let outp = resolve_path(root, &p.stdout_log);
        if let Ok(v) = tail_last_lines(&outp, lines) {
            for line in v {
                println!("[{}] {}", p.name, line);
            }
        } else {
            println!("[{}] (no stdout log yet at {})", p.name, outp);
        }
        let errp = resolve_path(root, &p.stderr_log);
        if let Ok(v) = tail_last_lines(&errp, lines) {
            for line in v {
                println!("[{} ERR] {}", p.name, line);
            }
        } else {
            println!("[{} ERR] (no stderr log yet at {})", p.name, errp);
        }
    }
    Ok(())
}

fn tail_last_lines(path: &str, n: usize) -> Result<Vec<String>> {
    use std::fs::File;
    use std::io::{Read, Seek, SeekFrom};
    let mut f = File::open(path)?;
    let mut buf: Vec<u8> = Vec::new();
    let file_size = f.metadata()?.len() as i64;
    let mut read_size: i64 = 0;
    let chunk: i64 = 8192;
    while file_size - read_size > 0 {
        let to_read = if file_size - read_size >= chunk {
            chunk
        } else {
            file_size - read_size
        };
        read_size += to_read;
        f.seek(SeekFrom::Start((file_size - read_size) as u64))?;
        let mut temp = vec![0u8; to_read as usize];
        f.read_exact(&mut temp)?;
        buf.splice(0..0, temp); // prepend
        let newline_count = bytecount::count(&buf, b'\n');
        if newline_count as usize > n {
            break;
        }
        if read_size >= file_size {
            break;
        }
    }
    let s = String::from_utf8_lossy(&buf);
    let mut lines_vec: Vec<&str> = s.split('\n').collect();
    if lines_vec.last().map(|x| x.is_empty()).unwrap_or(false) {
        lines_vec.pop();
    }
    let take = if lines_vec.len() > n {
        lines_vec[lines_vec.len() - n..].to_vec()
    } else {
        lines_vec
    };
    Ok(take.into_iter().map(|s| s.to_string()).collect())
}

fn follow_combined(
    processes: Vec<ProcessInfo>,
    lines: usize,
    root: &std::path::Path,
) -> Result<()> {
    use tokio::runtime::Runtime;
    use tokio::sync::mpsc;

    let rt = Runtime::new()?;
    rt.block_on(async move {
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();

        // Print initial tails
        for p in &processes {
            let outp = resolve_path(root, &p.stdout_log);
            if let Ok(v) = tail_last_lines(&outp, lines) {
                for line in v {
                    let _ = tx.send(format!("[{}] {}", p.name, line));
                }
            }
            let errp = resolve_path(root, &p.stderr_log);
            if let Ok(v) = tail_last_lines(&errp, lines) {
                for line in v {
                    let _ = tx.send(format!("[{} ERR] {}", p.name, line));
                }
            }
        }

        // Spawn followers for each file
        for p in &processes {
            let txo = tx.clone();
            let name = p.name.clone();
            let out = resolve_path(root, &p.stdout_log);
            tokio::spawn(async move {
                let _ = follow_file(out, format!("[{}] ", name), txo).await;
            });
            let txe = tx.clone();
            let namee = p.name.clone();
            let err = resolve_path(root, &p.stderr_log);
            tokio::spawn(async move {
                let _ = follow_file(err, format!("[{} ERR] ", namee), txe).await;
            });
        }

        // Print lines as they arrive; stop on Ctrl+C / signals
        #[cfg(unix)]
        {
            let mut sigint =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
            let mut sigterm =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
            loop {
                tokio::select! {
                    Some(line) = rx.recv() => { println!("{}", line); },
                    _ = sigint.recv() => { break; },
                    _ = sigterm.recv() => { break; }
                }
            }
        }
        #[cfg(not(unix))]
        {
            loop {
                tokio::select! {
                    Some(line) = rx.recv() => { println!("{}", line); },
                    _ = tokio::signal::ctrl_c() => { break; },
                }
            }
        }
        Ok::<(), anyhow::Error>(())
    })?;
    Ok(())
}

async fn follow_file(
    path: String,
    prefix: String,
    tx: tokio::sync::mpsc::UnboundedSender<String>,
) -> Result<()> {
    use tokio::fs::OpenOptions as AOpenOptions;
    use tokio::io::{AsyncReadExt, AsyncSeekExt};
    use tokio::time::{sleep, Duration};

    // Wait for file to exist
    let mut retries = 0;
    loop {
        if std::path::Path::new(&path).exists() {
            break;
        }
        if retries > 40 {
            return Ok(());
        }
        sleep(Duration::from_millis(250)).await;
        retries += 1;
    }

    let mut f = AOpenOptions::new().read(true).open(&path).await?;
    // Seek to end
    let mut pos = f.seek(std::io::SeekFrom::End(0)).await?;
    let mut buf = vec![0u8; 8192];
    let mut partial = String::new();

    loop {
        let n = f.read(&mut buf).await?;
        if n == 0 {
            sleep(Duration::from_millis(300)).await;
            // If file truncated, reset position
            let len = f.metadata().await?.len();
            if len < pos {
                pos = f.seek(std::io::SeekFrom::End(0)).await?;
            }
            continue;
        }
        pos += n as u64;
        let chunk = String::from_utf8_lossy(&buf[..n]);
        partial.push_str(&chunk);
        while let Some(idx) = partial.find('\n') {
            let line = partial[..idx].to_string();
            let _ = tx.send(format!("{}{}", prefix, line));
            partial = partial[idx + 1..].to_string();
        }
    }
}
