# oxproc

A simple Rust-based process manager that reads a list of long-running processes from a configuration file, starts them in the background, and collects their logs.

## Features

-   Supports configuration via `proc.toml` (preferred) or a standard `Procfile`.
-   Streams all process logs to the console with prefixes by default (foreground/dev mode).
-   Daemon mode via `start` subcommand, writing `stdout` and `stderr` to per‑process files.
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

### Global option: --root

All commands accept `--root <path>` to point oxproc at a different project directory (where `proc.toml`/`Procfile` live). Defaults to current directory.

Examples:

```sh
./target/release/oxproc --root /path/to/project start
./target/release/oxproc --root .. status
./target/release/oxproc --root /path/to/project logs -f
```

### Foreground (dev) mode

To monitor the output of all processes in real time (no daemon), run:

```sh
./target/release/oxproc
```

Press `Ctrl+C` to shut down children.

### Daemon mode

Start a background manager that daemonizes and writes state under `$XDG_STATE_HOME/oxproc/<project-id>/`:

```sh
./target/release/oxproc start
```

When you start, oxproc prints where it writes state and logs, for quick diagnostics, e.g.:

```
Starting oxproc daemon for /path/to/project
State: /home/user/.local/state/oxproc/<project-id>
PID file: /home/user/.local/state/oxproc/<project-id>/manager.pid
Manager log: /home/user/.local/state/oxproc/<project-id>/manager.log
```

Follow logs immediately after starting (combined view):

```sh
./target/release/oxproc start -f
```

Check status of the daemonized processes:

```sh
./target/release/oxproc status
```

Stop all processes for this project (sends SIGTERM, then SIGKILL after a grace period):

```sh
./target/release/oxproc stop --grace 5
```

Show log file locations or follow (combined view supported):

```sh
./target/release/oxproc logs            # prints tail for all processes (stdout + stderr)
./target/release/oxproc logs -f         # combined tail -f for all processes
./target/release/oxproc logs -n 200     # last 200 lines (no follow)
./target/release/oxproc logs --name web -f   # follow only a single process

### Restart

Stop then start in one command. You can add `-f` to attach to logs after restart:

```sh
./target/release/oxproc restart               # stop then start
./target/release/oxproc restart --grace 5 -f  # grace period and follow logs
```
```

Notes
- oxproc cleans up a stale `manager.pid` automatically if it detects the manager is not running.
- State files live under `$XDG_STATE_HOME/oxproc/<project-id>/` (default `~/.local/state/oxproc/...`).

### Tasks (proc.toml only)

When using `proc.toml`, oxproc can run one‑off tasks defined under a `[tasks]` table.

Example `proc.toml` snippet:

```toml
[web]
cmd = "npm run dev"

[tasks.build]
cmd = "cargo build"

[tasks.test]
cmd = "cargo test"
```

Run tasks:

```sh
oxproc run frontend:build      # runs [tasks.frontend.build]
oxproc frontend:build          # shorthand (external subcommand)
oxproc run api:migrate -- --dry-run
```

Notes
- Tasks are only available with `proc.toml`. When using a legacy `Procfile`, `oxproc run <task>` and `oxproc <task>` are not supported.
- Tasks execute as foreground one‑offs and inherit stdio; they do not use the daemon or log files.
- If an entry in `proc.toml` does not have a `tasks.` prefix, it is treated as a process (backwards compatible with existing configs).
- You can still invoke with dots (e.g., `frontend.build`), but colons are preferred for CLI usage and listing.

### List processes and tasks

Show configured processes and (when using `proc.toml`) tasks:

```sh
oxproc list              # human output
oxproc ls --json         # machine output
oxproc list --names-only # names only (both processes and tasks)
oxproc list --tasks-only # only tasks (proc.toml only)
```

## License

This project is licensed under the MIT License.
