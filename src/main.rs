use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod color;
mod config;
#[cfg(unix)]
mod daemon;
mod dirs;
mod list;
mod manager;
mod state;
mod task;

// config loader is used via config::load_config_from

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Project root containing proc.toml/Procfile (default: current dir)
    #[arg(global = true, long = "root", value_name = "PATH")]
    root: Option<PathBuf>,

    /// Colorize output: auto, always, or never
    #[arg(global = true, long = "color", value_enum)]
    color: Option<ColorChoice>,

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
    #[command(alias = "ps")]
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
    /// List configured processes and tasks (proc.toml only for tasks)
    #[command(alias = "ls")]
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Print names only, one per line
        #[arg(long = "names-only")]
        names_only: bool,
        /// Show only processes
        #[arg(long = "processes-only")]
        processes_only: bool,
        /// Show only tasks
        #[arg(long = "tasks-only")]
        tasks_only: bool,
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

#[derive(Clone, Debug, clap::ValueEnum)]
enum ColorChoice {
    Auto,
    Always,
    Never,
}

impl From<ColorChoice> for color::ColorMode {
    fn from(c: ColorChoice) -> Self {
        match c {
            ColorChoice::Auto => color::ColorMode::Auto,
            ColorChoice::Always => color::ColorMode::Always,
            ColorChoice::Never => color::ColorMode::Never,
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    color::init(cli.color.map(|c| c.into()));
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
        Some(Commands::List {
            json,
            names_only,
            processes_only,
            tasks_only,
        }) => {
            let info = list::gather_list_info(&root)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&info)?);
                return Ok(());
            }
            if names_only {
                let s = list::format_list_names_only(&info, processes_only, tasks_only);
                if !s.is_empty() {
                    println!("{}", s);
                }
                return Ok(());
            }
            let s = list::format_list_human(&info, processes_only, tasks_only);
            print!("{}", s);
            Ok(())
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
                    let p = color::prefix(&child_name);
                    println!("{}{}{}", p, prefix, line);
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
    use tokio::runtime::Runtime;

    // Gate: only available for proc.toml projects
    match config::detect_source(root)? {
        config::ConfigSource::Procfile => {
            anyhow::bail!("Task runner requires proc.toml. Current project uses a Procfile.");
        }
        config::ConfigSource::ProcToml => {}
    }

    let tasks_opt = config::load_tasks_from(root)?;
    let tasks = tasks_opt.unwrap_or_default();

    // Normalize user query: allow frontend:build or frontend.build
    let key = task::normalize_task_query(task);

    let Some(_) = tasks.get(&key) else {
        let mut available: Vec<String> = tasks.keys().map(|k| task::display_task_name(k)).collect();
        available.sort();
        if available.is_empty() {
            anyhow::bail!("Unknown task '{}'. No tasks defined under [tasks].", task);
        } else {
            anyhow::bail!(
                "Unknown task '{}'. Available tasks: {}",
                task,
                available.join(", ")
            );
        }
    };

    // Execute task graph
    let rt = Runtime::new()?;
    let outcome = rt.block_on(async move {
        exec_task(
            root,
            &tasks,
            &key,
            args,
            &mut Vec::new(),
            StdioMode::Inherit,
        )
        .await
    })?;

    match outcome {
        ExecOutcome::Success => Ok(()),
        ExecOutcome::Failed(code) => {
            std::process::exit(code);
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum StdioMode<'a> {
    Inherit,
    Prefixed(&'a str),
}

#[derive(Debug)]
enum ExecOutcome {
    Success,
    Failed(i32),
}

type ExecFut<'a> = futures::future::BoxFuture<'a, Result<ExecOutcome>>;

fn exec_task<'a>(
    root: &'a std::path::Path,
    tasks: &'a std::collections::HashMap<String, config::TaskConfig>,
    name: &'a str,
    args: &'a [String],
    stack: &'a mut Vec<String>,
    stdio: StdioMode<'a>,
) -> ExecFut<'a> {
    Box::pin(async move {
        use crate::config::TaskKind;

        let Some(task_cfg) = tasks.get(name) else {
            let mut available: Vec<String> =
                tasks.keys().map(|k| task::display_task_name(k)).collect();
            available.sort();
            anyhow::bail!(
                "Unknown task '{}'. Available tasks: {}",
                task::display_task_name(name),
                available.join(", ")
            );
        };

        // Cycle detection
        if stack.contains(&name.to_string()) {
            stack.push(name.to_string());
            let pretty = stack
                .iter()
                .map(|s| task::display_task_name(s))
                .collect::<Vec<_>>()
                .join(" -> ");
            anyhow::bail!("Dependency cycle detected: {}", pretty);
        }

        stack.push(name.to_string());

        let result = match &task_cfg.kind {
            TaskKind::Shell { cmd, cwd } => {
                run_shell_task(root, name, cmd, cwd.as_deref(), args, stdio).await?
            }
            TaskKind::Composite { children, parallel } => {
                if *parallel {
                    // Launch all children concurrently, each with prefixed output using the top-level child label.
                    let mut futs = Vec::new();
                    for c in children {
                        let child_abs = task::resolve_child_name(name, c);
                        let display = task::display_task_name(&child_abs);
                        let mut local_stack = stack.clone();
                        let args_vec = args.to_vec();
                        let fut = async move {
                            exec_task(
                                root,
                                tasks,
                                &child_abs,
                                &args_vec,
                                &mut local_stack,
                                StdioMode::Prefixed(&display),
                            )
                            .await
                        };
                        futs.push(fut);
                    }
                    let results = futures::future::join_all(futs).await;
                    // If any child failed, propagate first non-zero code
                    let mut first_failed: Option<i32> = None;
                    for r in results {
                        match r? {
                            ExecOutcome::Success => {}
                            ExecOutcome::Failed(code) => {
                                if first_failed.is_none() {
                                    first_failed = Some(code);
                                }
                            }
                        }
                    }
                    match first_failed {
                        Some(code) => ExecOutcome::Failed(code),
                        None => ExecOutcome::Success,
                    }
                } else {
                    // Sequential: run in order, stop on first failure
                    for c in children {
                        let child_abs = task::resolve_child_name(name, c);
                        println!("▶ running {}…", task::display_task_name(&child_abs));
                        match exec_task(root, tasks, &child_abs, args, stack, stdio).await? {
                            ExecOutcome::Success => {}
                            ExecOutcome::Failed(code) => return Ok(ExecOutcome::Failed(code)),
                        }
                    }
                    ExecOutcome::Success
                }
            }
        };

        stack.pop();
        Ok(result)
    })
}

async fn run_shell_task(
    root: &std::path::Path,
    name: &str,
    cmd_str: &str,
    cwd: Option<&str>,
    args: &[String],
    stdio: StdioMode<'_>,
) -> Result<ExecOutcome> {
    use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};

    // Build final command string
    let mut final_cmd = cmd_str.to_string();
    if !args.is_empty() {
        let extra = args.join(" ");
        final_cmd.push(' ');
        final_cmd.push_str(&extra);
    }

    let mut cmd = tokio::process::Command::new("sh");
    cmd.arg("-c").arg(&final_cmd);

    // cwd handling
    if let Some(cwd) = cwd {
        let abs = if std::path::Path::new(cwd).is_absolute() {
            std::path::PathBuf::from(cwd)
        } else {
            root.join(cwd)
        };
        if !abs.exists() {
            anyhow::bail!(
                "Task '{}' cwd does not exist: {}",
                task::display_task_name(name),
                abs.display()
            );
        }
        cmd.current_dir(abs);
    } else {
        cmd.current_dir(root);
    }

    match stdio {
        StdioMode::Inherit => {
            use std::process::Stdio;
            cmd.stdin(Stdio::inherit());
            cmd.stdout(Stdio::inherit());
            cmd.stderr(Stdio::inherit());
            let status = cmd.status().await?;
            if !status.success() {
                if let Some(code) = status.code() {
                    return Ok(ExecOutcome::Failed(code));
                } else {
                    anyhow::bail!("Task terminated by signal");
                }
            }
            Ok(ExecOutcome::Success)
        }
        StdioMode::Prefixed(label) => {
            use std::process::Stdio;
            cmd.stdin(Stdio::null());
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());
            let mut child = cmd.spawn()?;
            let prefix = color::prefix(label);

            async fn handle_output<T: AsyncRead + Unpin>(prefix: String, stream: T, err: bool) {
                let mut reader = BufReader::new(stream).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    if err {
                        println!("{}[ERR] {}", prefix, line);
                    } else {
                        println!("{}{}", prefix, line);
                    }
                }
            }

            let mut handles = Vec::new();
            if let Some(stdout) = child.stdout.take() {
                handles.push(tokio::spawn(handle_output(prefix.clone(), stdout, false)));
            }
            if let Some(stderr) = child.stderr.take() {
                handles.push(tokio::spawn(handle_output(prefix.clone(), stderr, true)));
            }

            let status = child.wait().await?;
            futures::future::join_all(handles).await;
            if !status.success() {
                if let Some(code) = status.code() {
                    return Ok(ExecOutcome::Failed(code));
                } else {
                    anyhow::bail!("Task terminated by signal");
                }
            }
            Ok(ExecOutcome::Success)
        }
    }
}
