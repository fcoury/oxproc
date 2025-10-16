use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod config;
#[cfg(unix)]
mod daemon;
mod dirs;
mod manager;
mod state;

// configuration is loaded via config::load_project_config

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
    /// Run all processes in the foreground (legacy dev mode)
    Dev {},
    /// Run a single task defined in proc.toml
    Run {
        /// Task name to execute
        #[arg(value_name = "TASK")]
        task: String,
    },
    #[command(external_subcommand)]
    ImplicitTask(Vec<String>),
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
        Some(Commands::Dev {}) => tokio_foreground_follow(&root),
        Some(Commands::Run { task }) => run_task_command(&root, &task),
        Some(Commands::ImplicitTask(values)) => handle_implicit_task(&root, values),
        None => handle_default_task(&root),
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

fn run_task_command(root: &std::path::Path, task_name: &str) -> Result<()> {
    let project = config::load_project_config(root)?;
    if project.kind != config::ConfigKind::ProcToml {
        anyhow::bail!(
            "Tasks are only supported when using proc.toml. Use `oxproc dev` to run processes."
        );
    }

    let tasks = &project.tasks;
    if tasks.is_empty() {
        anyhow::bail!(
            "No tasks defined in proc.toml. Add a [tasks] section or use `oxproc dev`."
        );
    }

    if let Some(task) = tasks.iter().find(|task| task.name == task_name) {
        execute_task(root, task.clone())
    } else {
        let available = tasks
            .iter()
            .map(|task| task.name.clone())
            .collect::<Vec<_>>();
        if available.is_empty() {
            anyhow::bail!(
                "Task '{}' not found and no tasks are defined in proc.toml.",
                task_name
            );
        } else {
            anyhow::bail!(
                "Task '{}' not found. Available tasks: {}",
                task_name,
                available.join(", ")
            );
        }
    }
}

fn handle_implicit_task(root: &std::path::Path, values: Vec<String>) -> Result<()> {
    if values.is_empty() {
        return handle_default_task(root);
    }

    if values.len() > 1 {
        anyhow::bail!(
            "Task invocation via `oxproc <task>` does not support additional arguments. Use `oxproc run {}`.",
            values[0]
        );
    }

    run_task_command(root, &values[0])
}

fn handle_default_task(root: &std::path::Path) -> Result<()> {
    let project = config::load_project_config(root)?;
    let kind = project.kind;
    let tasks = project.tasks;

    match kind {
        config::ConfigKind::ProcToml => match tasks.len() {
            0 => anyhow::bail!(
                "No tasks defined in proc.toml. Use `oxproc dev` or add tasks under a [tasks] table."
            ),
            1 => execute_task(root, tasks.into_iter().next().unwrap()),
            _ => {
                let names = tasks
                    .iter()
                    .map(|task| task.name.clone())
                    .collect::<Vec<_>>();
                anyhow::bail!(
                    "Multiple tasks defined ({}). Specify one with `oxproc run <task>` or `oxproc <task>`.",
                    names.join(", ")
                );
            }
        },
        config::ConfigKind::Procfile => {
            anyhow::bail!(
                "Tasks are not supported for Procfile-only projects. Use `oxproc dev` to run processes or switch to proc.toml with a [tasks] section."
            );
        }
    }
}

fn execute_task(root: &std::path::Path, task: config::TaskConfig) -> Result<()> {
    use std::process::Stdio;
    use std::sync::Arc;
    use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
    use tokio::process::Command;
    use tokio::runtime::Runtime;
    use tokio::sync::Mutex;

    let task_name = task.name.clone();
    let display_name = task_name.clone();
    let command = task.command.clone();
    let cwd = task.cwd.clone();
    let root = root.to_path_buf();

    let rt = Runtime::new()?;
    let status = rt.block_on(async move {
        let task_name = task_name;
        async fn handle_output<T: AsyncRead + Unpin>(
            child_name: String,
            stream: T,
            prefix: &'static str,
        ) {
            let mut reader = BufReader::new(stream).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                println!("[{}] {}{}", child_name, prefix, line);
            }
        }

        let mut cmd = Command::new("sh");
        cmd.arg("-c");
        cmd.arg(&command);

        let working_dir = if let Some(cwd) = cwd {
            let path = if std::path::Path::new(&cwd).is_absolute() {
                std::path::PathBuf::from(&cwd)
            } else {
                root.join(&cwd)
            };
            if !path.exists() {
                return Err(anyhow::anyhow!(
                    "Task '{}' cwd does not exist: {}",
                    task_name,
                    path.display()
                ));
            }
            path
        } else {
            root.clone()
        };
        cmd.current_dir(&working_dir);

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        println!("Running task '{}' (command: {})", task_name, command);

        let mut child = cmd.spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to capture stdout for task '{}'", task_name))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to capture stderr for task '{}'", task_name))?;

        let child = Arc::new(Mutex::new(child));
        let stdout_handle = tokio::spawn(handle_output(task_name.clone(), stdout, ""));
        let stderr_handle = tokio::spawn(handle_output(task_name.clone(), stderr, "[ERR] "));

        let child_for_wait = child.clone();

        let status = tokio::select! {
            status = async {
                let mut locked = child_for_wait.lock().await;
                locked.wait().await
            } => status?,
            _ = tokio::signal::ctrl_c() => {
                println!("\nReceived interrupt. Stopping task '{}'...", task_name);
                let mut locked = child.lock().await;
                locked.kill().await?;
                locked.wait().await
            }
        };

        let _ = stdout_handle.await;
        let _ = stderr_handle.await;

        Ok::<std::process::ExitStatus, anyhow::Error>(status)
    })?;

    if !status.success() {
        if let Some(code) = status.code() {
            std::process::exit(code);
        } else {
            eprintln!("Task '{}' terminated by signal.", display_name);
            std::process::exit(1);
        }
    }

    Ok(())
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
        let project = config::load_project_config(root)?;
        let configs = project.processes;

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
