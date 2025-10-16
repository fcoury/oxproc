# oxproc

A simple Rust-based process manager and lightweight task runner. It can supervise long-running processes defined in a configuration file, execute ad-hoc shell tasks, and collect logs for both foreground and daemonized workflows.

## Features

-   Supports configuration via `proc.toml` (preferred) or a standard `Procfile` for basic process lists.
-   `[tasks]` support in `proc.toml` for defining reusable shell commands or foreground process groups that can be invoked with `oxproc run <task>`.
-   Streams process logs in the foreground when a task targets processes, or runs everything in the background via the daemon.
-   Gracefully shuts down child processes on `Ctrl+C` and propagates signals during daemon shutdown.

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

`proc.toml` separates long-running services from ad-hoc tasks. Process definitions live under `[processes]`; reusable shell tasks (including foreground groupings of processes) live under `[tasks]`.

**Example `proc.toml`:**

```toml
[processes.web]
cmd = "python -m http.server 8000"
stdout = "logs/web.out.log"
stderr = "logs/web.err.log"

[processes.worker]
cmd = "while true; do echo 'Processing...'; sleep 2; done"
# stdout/stderr will default to logs/worker.out.log and logs/worker.err.log

[tasks.dev]
# Run both long-lived processes in the foreground (former default behaviour)
processes = ["web", "worker"]

[tasks.migrate]
# One-off shell command task
cmd = "poetry run alembic upgrade head"
cwd = "services/api"
```

-   Every entry under `[processes.<name>]` is available to the daemon (`start`, `restart`, etc.).
-   Tasks can either execute a standalone `cmd` (with optional `cwd`) or reference one or more process names via `processes`, causing `oxproc run <task>` to stream those processes in the foreground.
-   The `dev` task in the example restores the legacy behaviour where `oxproc` immediately streamed all processes; now you opt in explicitly with `oxproc run dev`.

### 2. `Procfile` (Fallback)

If `proc.toml` is not found, `oxproc` will look for a standard `Procfile`.

**Example `Procfile`:**

```
web: python -m http.server 8000
worker: while true; do echo 'Processing...'; sleep 2; done
```

`Procfile`-only setups do not support tasks—define a `proc.toml` if you need the `[tasks]` section.

## Usage

### CLI overview

```
$ oxproc --help
Usage: oxproc [OPTIONS] <COMMAND>

Commands:
  run       Run a named task from the [tasks] section of proc.toml
  start     Start all processes as a background daemon
  status    Show status for the current project's processes
  stop      Stop all processes for the current project
  logs      View or follow collected logs from the daemon
  restart   Stop then start all processes again
  help      Print this message or the help of the given subcommand(s)

Options:
  --root <PATH>  Project root containing proc.toml/Procfile (default: current dir)
  -h, --help     Print help
  -V, --version  Print version
```

Invoking `oxproc` with no subcommand now prints the help summary above. Use `oxproc run <task>` (or another subcommand) to perform work explicitly.

### Global option: --root

All commands accept `--root <path>` to point oxproc at a different project directory (where `proc.toml`/`Procfile` live). Defaults to current directory.

Examples:

```sh
./target/release/oxproc --root /path/to/project run dev
./target/release/oxproc --root .. status
./target/release/oxproc --root /path/to/project logs -f
```

### Running tasks & foreground sessions

Use `oxproc run <task>` to execute a task declared under `[tasks]` in `proc.toml`.

```sh
./target/release/oxproc run dev         # Streams the web + worker processes in the foreground
./target/release/oxproc run migrate     # Executes the one-off migration command
```

-   Tasks that specify `processes = ["..."]` reuse the corresponding process definitions and stream their output with prefixes until you press `Ctrl+C`.
-   Tasks that specify `cmd = "..."` run that shell command once, respecting optional `cwd` overrides.
-   Define whichever `tasks.*` entry you need for local development ergonomics or automation.

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

### Status

Check status of the daemonized processes:

```sh
./target/release/oxproc status
```

### Stop

Stop all processes for this project (sends SIGTERM, then SIGKILL after a grace period):

```sh
./target/release/oxproc stop --grace 5
```

### Logs

Show log file locations or follow (combined view supported):

```sh
./target/release/oxproc logs                # prints tail for all processes (stdout + stderr)
./target/release/oxproc logs -f             # combined tail -f for all processes
./target/release/oxproc logs -n 200         # last 200 lines (no follow)
./target/release/oxproc logs --name web -f  # follow only a single process
```

### Restart

Stop then start in one command. You can add `-f` to attach to logs after restart:

```sh
./target/release/oxproc restart               # stop then start
./target/release/oxproc restart --grace 5 -f  # grace period and follow logs
```

## Notes

-   oxproc cleans up a stale `manager.pid` automatically if it detects the manager is not running.
-   State files live under `$XDG_STATE_HOME/oxproc/<project-id>/` (default `~/.local/state/oxproc/...`).
-   Tasks are only loaded from `proc.toml`; they are ignored when using a plain `Procfile`.

## License

This project is licensed under the MIT License.
