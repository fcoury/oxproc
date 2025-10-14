### Plan

1.  **Project Setup & Dependencies**
    *   Initialize a new Rust binary project using `cargo new oxproc`.
    *   Add the necessary dependencies to `Cargo.toml`:
        *   `clap`: For robust command-line argument parsing (e.g., the `--follow` flag).
        *   `tokio`: As the core asynchronous runtime. It's ideal for managing multiple child processes and their I/O streams concurrently without blocking.
        *   `serde` & `toml`: To parse the preferred `proc.toml` configuration file into strongly-typed Rust structs.
        *   `anyhow` & `thiserror`: For ergonomic and clear error handling.

2.  **Configuration Loading (`config.rs`)**
    *   Define a unified `ProcessConfig` struct to represent a single process, containing its name (e.g., "web"), command, and optional, specific paths for its stdout and stderr logs.
    *   Implement a configuration loader that:
        *   First, searches for a `proc.toml` file in the current directory. If found, it will be parsed. This file will allow for richer configuration, like specifying exact log paths.
        *   If `proc.toml` is not found, it will fall back to searching for a standard `Procfile`.
        *   If neither file is found, the application will exit with a clear error message.
    *   The loader will return a `Vec<ProcessConfig>`, providing a consistent interface for the rest of the application.

3.  **Core Process Management (`main.rs`)**
    *   The main function will be asynchronous (`#[tokio::main]`).
    *   It will start by parsing the command-line arguments using `clap` to determine if "follow" mode is active.
    *   It will then call the configuration loader to get the list of processes to run.
    *   A `tokio::task::JoinSet` will be used to manage the lifecycle of all spawned tasks (child processes and their log handlers).

4.  **Log Handling & Process Execution**
    *   The application will iterate through each `ProcessConfig`.
    *   For each process, it will use `tokio::process::Command` to spawn the child process with its `stdout` and `stderr` streams piped for capture.
    *   Based on the mode:
        *   **Default (Background) Mode**:
            *   For each process, two new async tasks will be spawned: one for stdout and one for stderr.
            *   Each task will open a log file (e.g., `web-1.out.log`, `web-1.err.log`) in append mode.
            *   It will then read lines from the process stream and write them asynchronously to the corresponding file.
            *   The main application will print the process names and their PIDs to the console and then wait for a shutdown signal.
        *   **Follow Mode (`--follow`)**:
            *   Similarly, two async tasks will be spawned per process for stdout and stderr.
            *   Instead of writing to a file, these tasks will acquire a lock on the console's stdout.
            *   They will format each line with a distinct prefix (e.g., `[WEB] `) and print it to the screen. This provides a multiplexed, real-time view of all logs.

5.  **Graceful Shutdown**
    *   The main process will listen for a `Ctrl+C` signal (`SIGINT`).
    *   Upon receiving the signal, it will gracefully shut down all the child processes it spawned.
    *   The `JoinSet` will be used to ensure all tasks are properly awaited and cleaned up before the main application exits.
