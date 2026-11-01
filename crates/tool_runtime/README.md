# tool_runtime

Policy-gated tool execution for the Talos local control plane: registry,
adapters, gated executor, and evidence-backed receipts.

```sh
cargo test -p tool_runtime
```

## `process.exec` kill semantics (TAL-006)

Timed-out subprocesses are killed and reaped — never orphaned:

- Every spawn sets `kill_on_drop(true)` (drop-without-reap still kills).
- On timeout the executor explicitly calls `kill()` **before** returning.
- Process-tree kill follows: Unix children are `setsid`'d at spawn so
  `kill -KILL -<pgid>` takes the whole group (shell + grandchildren);
  Windows uses `taskkill /T /F`.
- Stdout/stderr are drained with timeouts (a live child holding a pipe open
  never stalls the kill path), then `wait()` reaps the zombie.
- Kill failures are logged, never fatal — the timeout outcome stands either
  way.
- Every shell receipt embeds `processLifecycle`: `killedOnTimeout`,
  `killMethod` (`direct` | `process_group` | `taskkill`, `None` when no kill
  was needed), `reapConfirmed`.

Timeouts surface as `Failed` + `timeout` receipts (`timedOut: true`), so a
timed-out command never looks successful.
