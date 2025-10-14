# oxproc

A simple Rust-based process manager that reads a list of long-running processes from a configuration file, starts them in the background, and collects their logs.

## Features

-   Supports configuration via `proc.toml` (preferred) or a standard `Procfile`.
-   Runs processes in the background by default, logging `stdout` and `stderr` to separate files.
-   Provides a `--follow` mode to stream all process logs to the console with prefixes.
-   Gracefully shuts down all child processes on `Ctrl+C`.

## Installation

1.  **Install Rust**: If you don't have Rust installed, get it from [rust-lang.org](https://www.rust-lang.org/).
2.  **Clone the repository**:
    ```sh
    git clone <repository-url>
    cd oxproc
    ```
3.  **Build the project**:
    ```sh
    cargo build --release
    ```
    The executable will be located at `target/release/oxproc`.

## Configuration

`oxproc` looks for a configuration file in the current directory in the following order:

### 1. `proc.toml` (Preferred)

This is the recommended way to configure `oxproc` as it allows for more detailed control, such as specifying custom log file paths.

**Example `proc.toml`:**

```toml
[web]
cmd = "python -m http.server 8000"
stdout = "logs/web.out.log"
stderr = "logs/web.err.log"

[worker]
cmd = "while true; do echo 'Processing...'; sleep 2; done"
# stdout and stderr will default to worker.out.log and worker.err.log
```

### 2. `Procfile` (Fallback)

If `proc.toml` is not found, `oxproc` will look for a standard `Procfile`.

**Example `Procfile`:**

```
web: python -m http.server 8000
worker: while true; do echo 'Processing...'; sleep 2; done
```

When using a `Procfile`, log files will be automatically named (e.g., `web.out.log`, `web.err.log`).

## Usage

Run `oxproc` from the directory containing your configuration file (`proc.toml` or `Procfile`).

### Background Mode (Default)

To start all processes in the background, simply run the executable. Logs will be written to the configured files.

```sh
./target/release/oxproc
```

You will see output indicating that the processes have started, and you can then safely close the terminal. The processes will continue to run.

### Follow Mode

To monitor the output of all running processes in real-time, use the `--follow` flag. All logs will be streamed to your console, prefixed with the process name.

```sh
./target/release/oxproc --follow
```

Press `Ctrl+C` to shut down `oxproc` and all the child processes.

## License

This project is licensed under the MIT License.
