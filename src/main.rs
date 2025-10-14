use anyhow::Result;
use clap::Parser;
use futures::future::join_all;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader, AsyncRead};
use tokio::process::Command;
use tokio::sync::Mutex;
use std::sync::Arc;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;

mod config;
use config::load_config;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Follow the output of the processes
    #[arg(short, long)]
    follow: bool,
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

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let configs = load_config()?;

    let mut children = Vec::new();
    let mut handles = Vec::new();

    for config in configs {
        let mut cmd = Command::new("sh");
        cmd.arg("-c");
        cmd.arg(&config.command);
        if let Some(cwd) = &config.cwd {
            cmd.current_dir(cwd);
        }
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = cmd.spawn()?;
        let pid = child.id().unwrap();
        println!("Started {} with PID: {}", config.name, pid);

        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        let stdout_log = config.stdout_log.clone().unwrap_or_else(|| format!("{}.out.log", config.name));
        let stderr_log = config.stderr_log.clone().unwrap_or_else(|| format!("{}.err.log", config.name));

        let stdout_handle = tokio::spawn(handle_output(
            config.name.clone(),
            stdout,
            Some(stdout_log),
            args.follow,
            "",
        ));

        let stderr_handle = tokio::spawn(handle_output(
            config.name.clone(),
            stderr,
            Some(stderr_log),
            args.follow,
            "[ERR] ",
        ));
        
        children.push(Arc::new(Mutex::new(child)));
        handles.push(stdout_handle);
        handles.push(stderr_handle);
    }

    if args.follow {
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
    } else {
        println!("Processes are running in the background. Logs are being written to files.");
        tokio::signal::ctrl_c().await?;
        println!("\nShutting down...");
        for child in children.iter_mut() {
            let mut child_guard = child.lock().await;
            child_guard.kill().await?;
        }
    }

    Ok(())
}