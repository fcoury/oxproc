### Daemon Mode Plan (End-to-End)

1.  CLI Reshape (done)
    *   Add subcommands: `start`, `status`, `stop`, `logs`.
    *   Keep default foreground follow (no subcommand) for dev.

2.  State & Dirs (done)
    *   Compute state dir: `$XDG_STATE_HOME/oxproc/<hash-of-cwd>/`.
    *   Persist `state.json`, `manager.pid`, `manager.log`, `manager.lock`.

3.  Daemonization (done MVP)
    *   `start` uses `daemonize` to detach, redirect stdout/err to `manager.log`.
    *   Acquire exclusive lock to prevent multi-daemon for same project.

4.  Manager (MVP implemented)
    *   Spawn each process in its own session/process group (setsid).
    *   Stream stdout/stderr to per-process files (existing defaults preserved).
    *   Write `state.json` with manager + process metadata.
    *   Handle SIGINT/SIGTERM: send SIGTERM to each PGID, escalate to SIGKILL after 5s.

5.  Status/Stop/Logs (done for MVP)
    *   `status`: reads `state.json`, probes liveness, prints table.
    *   `stop`: sends SIGTERM->SIGKILL to process groups, then manager.
    *   `logs`: tail last N lines and support `-f/--follow` with combined prefixed view; filter by `--name`.

6.  Next Steps (TODO)
    *   Harden stale state recovery and lock handling (stale pid cleanup implemented; add stale lock detection).
    *   Add JSON output for `status`.
    *   Unit/integration tests for start/status/stop lifecycle.

7.  Phase 2 (Future)
    *   Add Unix socket control plane for richer status/log streaming.
    *   Restart policies (always/on-failure/backoff).
