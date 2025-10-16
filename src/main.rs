use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod config;
#[cfg(unix)]
mod daemon;
mod dirs;
mod manager;
mod state;

// config loader is used via config::load_config_from

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Project root containing proc.toml/Procfile (default: current dir)
    #[arg(global = true, long = "root", value_name = "PATH")]
    root: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Start all processes as a background daemon
    Start {
        /// Follow logs after starting (equivalent to: start && logs -f)
        #[arg(short, long)]
        follow: bool,
    },
    /// Show status for the current project's processes
    Status {},
    /// Stop all processes for the current project
    Stop {
        /// Grace period in seconds before SIGKILL
        #[arg(long, default_value_t = 5)]
        grace: u64,
    },
    /// Restart all processes (stop then start). Add -f to follow logs.
    Restart {
        /// Grace period in seconds before SIGKILL
        #[arg(long, default_value_t = 5)]
        grace: u64,
        /// Follow logs after restarting
        #[arg(short, long)]
        follow: bool,
    },
    /// View logs. By default shows combined logs. Use --name to filter.
    Logs {
        /// Process name to filter
        #[arg(long)]
        name: Option<String>,
        /// Follow the logs
        #[arg(short, long)]
        follow: bool,
        /// Number of lines from the end
        #[arg(short = 'n', long, default_value_t = 100)]
        lines: usize,
    },
    /// Run a one-off task from proc.toml
    Run {
        /// Task name under [tasks.<name>]
        task: String,
        /// Arguments passed to the task command after '--'
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
    /// Shorthand: if not a known command, treat first token as a task name
    #[command(external_subcommand)]
    External(Vec<String>),
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let root = cli.root.unwrap_or_else(|| std::env::current_dir().unwrap());
    match cli.command {
        Some(Commands::Start { follow }) => {
            #[cfg(unix)]
            {
                if follow {
                    start_and_follow(&root)
                } else {
                    daemon::start_daemon(&root)
                }
            }
            #[cfg(not(unix))]
            {
                anyhow::bail!("Daemon mode is only supported on Unix (Linux/macOS)");
            }
        }
        Some(Commands::Status {}) => {
            state::print_status(&root)?;
            Ok(())
        }
        Some(Commands::Stop { grace }) => {
            #[cfg(unix)]
            {
                manager::stop_all(&root, Some(std::time::Duration::from_secs(grace)))?;
                Ok(())
            }
            #[cfg(not(unix))]
            {
                anyhow::bail!("Stop is only supported on Unix in daemon mode");
            }
        }
        Some(Commands::Logs {
            name,
            follow,
            lines,
        }) => {
            manager::print_logs(&root, name, follow, lines)?;
            Ok(())
        }
        Some(Commands::Restart { grace, follow }) => {
            #[cfg(unix)]
            {
                manager::stop_all(&root, Some(std::time::Duration::from_secs(grace)))?;
                if follow {
                    start_and_follow(&root)
                } else {
                    daemon::start_daemon(&root)
                }
            }
            #[cfg(not(unix))]
            {
                anyhow::bail!("Restart is only supported on Unix in daemon mode");
            }
        }
        Some(Commands::Run { task, args }) => run_task(&root, &task, &args),
        Some(Commands::External(v)) => {
            if v.is_empty() {
                anyhow::bail!("No task name provided")
            } else {
                let task = &v[0];
                let args = v[1..].to_vec();
                run_task(&root, task, &args)
            }
        }
        None => {
            // Default: foreground follow of all processes (dev UX)
            tokio_foreground_follow(&root)
        }
    }
}

#[cfg(unix)]
fn start_and_follow(root: &std::path::Path) -> Result<()> {
    use std::process::Command;
    use std::time::Duration;

    // Spawn a fresh `oxproc start` without --follow to perform the daemonization
    let exe = std::env::current_exe()?;
    let mut args: Vec<String> = Vec::new();
    // forward --root if provided
    args.push("start".to_string());
    // If the user passed --root in the original invocation, `root` will reflect it; we must forward
    // by comparing with current_dir and adding explicit flag only if different.
    if let Ok(cwd) = std::env::current_dir() {
        if cwd != root {
            args.splice(
                0..0,
                vec!["--root".to_string(), root.to_string_lossy().to_string()],
            );
        }
    }

    let status = Command::new(exe)
        .args(&args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .spawn();

    match status {
        Ok(_child) => {
            // Wait for readiness then attach logs
            println!("Waiting for manager to become ready…");
            state::wait_for_manager_ready(root, Duration::from_secs(10))?;
            println!("Attaching to logs (Ctrl+C to detach)…");
            manager::print_logs(root, None, true, 100)?;
            Ok(())
        }
        Err(e) => {
            anyhow::bail!("Failed to spawn start: {}", e);
        }
    }
}

fn tokio_foreground_follow(root: &std::path::Path) -> Result<()> {
    use futures::future::join_all;
    use std::process::Stdio;
    use std::sync::Arc;
    use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
    use tokio::process::Command;
    use tokio::runtime::Runtime;
    use tokio::sync::Mutex;

    let rt = Runtime::new()?;
    rt.block_on(async move {
        let configs = config::load_config_from(root)?;

        async fn handle_output<T: AsyncRead + Unpin>(
            child_name: String,
            stream: T,
            _log_path: Option<String>,
            follow: bool,
            prefix: &'static str,
        ) {
            let mut reader = BufReader::new(stream).lines();
            while let Some(line) = reader.next_line().await.unwrap() {
                if follow {
                    println!("[{}] {}{}", child_name, prefix, line);
                }
            }
        }

        let mut children = Vec::new();
        let mut handles = Vec::new();

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

            let mut child = cmd.spawn()?;
            let pid = child.id().unwrap();
            println!("Started {} with PID: {}", config.name, pid);

            let stdout = child.stdout.take().unwrap();
            let stderr = child.stderr.take().unwrap();

            let stdout_handle =
                tokio::spawn(handle_output(config.name.clone(), stdout, None, true, ""));

            let stderr_handle = tokio::spawn(handle_output(
                config.name.clone(),
                stderr,
                None,
                true,
                "[ERR] ",
            ));

            children.push(Arc::new(Mutex::new(child)));
            handles.push(stdout_handle);
            handles.push(stderr_handle);
        }

        tokio::select! {
            _ = join_all(handles) => {},
            _ = tokio::signal::ctrl_c() => {
                println!("\nShutting down...");
                for child in children.iter_mut() {
                    let mut child_guard = child.lock().await;
                    child_guard.kill().await?;
                }
            }
        }

        Ok::<(), anyhow::Error>(())
    })?;

    Ok(())
}

fn run_task(root: &std::path::Path, task: &str, args: &[String]) -> Result<()> {
    use std::process::Stdio;
    use tokio::runtime::Runtime;

    // Gate: only available for proc.toml projects
    match config::detect_source(root)? {
        config::ConfigSource::Procfile => {
            anyhow::bail!(
                "Task runner requires proc.toml. Current project uses a Procfile."
            );
        }
        config::ConfigSource::ProcToml => {}
    }

    let tasks_opt = config::load_tasks_from(root)?;
    let tasks = tasks_opt.unwrap_or_default();
    let Some(t) = tasks.get(task) else {
        let available: Vec<String> = tasks.keys().cloned().collect();
        if available.is_empty() {
            anyhow::bail!(
                "Unknown task '{}'. No tasks defined under [tasks].",
                task
            );
        } else {
            anyhow::bail!(
                "Unknown task '{}'. Available tasks: {}",
                task,
                available.join(", ")
            );
        }
    };

    // Build final command string: task cmd + args joined by spaces
    let mut final_cmd = t.cmd.clone();
    if !args.is_empty() {
        let extra = args.join(" ");
        final_cmd.push(' ');
        final_cmd.push_str(&extra);
    }

    let rt = Runtime::new()?;
    rt.block_on(async move {
        let mut cmd = tokio::process::Command::new("sh");
        cmd.arg("-c").arg(final_cmd);

        // Handle cwd: absolute or relative to project root
        if let Some(cwd) = &t.cwd {
            let abs = if std::path::Path::new(cwd).is_absolute() {
                std::path::PathBuf::from(cwd)
            } else {
                root.join(cwd)
            };
            if !abs.exists() {
                anyhow::bail!("Task '{}' cwd does not exist: {}", t.name, abs.display());
            }
            cmd.current_dir(abs);
        } else {
            cmd.current_dir(root);
        }

        cmd.stdin(Stdio::inherit());
        cmd.stdout(Stdio::inherit());
        cmd.stderr(Stdio::inherit());

        let status = cmd.status().await?;
        if !status.success() {
            if let Some(code) = status.code() {
                std::process::exit(code);
            } else {
                anyhow::bail!("Task terminated by signal");
            }
        }
        Ok(())
    })
}
