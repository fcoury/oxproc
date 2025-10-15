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
    Start {},
    /// Show status for the current project's processes
    Status {},
    /// Stop all processes for the current project
    Stop {
        /// Grace period in seconds before SIGKILL
        #[arg(long, default_value_t = 5)]
        grace: u64,
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
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let root = cli.root.unwrap_or_else(|| std::env::current_dir().unwrap());
    match cli.command {
        Some(Commands::Start {}) => {
            #[cfg(unix)]
            return daemon::start_daemon(&root);
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
        None => {
            // Default: foreground follow of all processes (dev UX)
            tokio_foreground_follow(&root)
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
