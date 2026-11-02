# Talos CLI (`talos`)

Headless, scriptable access to the same policy broker and evidence ledger
the desktop app uses. Every mutating path evaluates through
`policy_broker::DefaultPolicyEngine` first — **the CLI cannot bypass any
safety gate**: a headless run that the broker does not `Allow` exits with
code 2 instead of executing.

Build: `cargo build -p talos_cli` → `crates/target/debug/talos[.exe]`
(release sidecars are staged per triple by
`scripts/stage-cli-binaries.mjs`; see `docs/INSTALL.md`).

## Exit codes (stable contract)

| Code | Meaning                                        |
| ---- | ---------------------------------------------- |
| 0    | Success                                        |
| 1    | Failure (bad input, missing record, I/O error) |
| 2    | Policy denied (broker verdict was not `Allow`) |

## Output formats

- `--format text` (default): human-readable lines.
- `--format json`: **JSONL** — one JSON object per line on stdout, each with
  `type` and `timestamp`. Parsers must ignore unknown fields (forward
  compatibility) and treat stderr as diagnostics, never events.

## JSONL event contract

| `type`              | Emitted when            | Key fields                                    |
| ------------------- | ----------------------- | --------------------------------------------- |
| `task_started`      | Queue entry created     | `task_id`, `goal`, `model`, `budget_usd`      |
| `policy_evaluated`  | Broker verdict recorded | `task_id`, `request_id`, `decision`, `reason` |
| `tool_invoked`      | Bounded probe step      | `task_id`, `tool_id`, `dry_run`, `status`     |
| `evidence_recorded` | Run receipt ledgered    | `task_id`, `evidence_id`                      |
| `cost_update`       | Spend snapshot          | `task_id`, `spend_usd`, `budget_usd`          |
| `task_completed`    | Terminal success        | `task_id`, `status`, `fidelity`               |
| `task_failed`       | Terminal failure/denial | `task_id`, `error`, `reason?`                 |
| `error`             | CLI-level failure       | `message`                                     |

Example (CI parsing):

```sh
export TALOS_DATA_DIR=/tmp/talos-ci
talos --format json run "summarize the ledger" | while read -r line; do
  type=$(echo "$line" | jq -r .type)
  [ "$type" = task_failed ] && exit 1
  [ "$type" = task_completed ] && exit 0
done
```

## Command reference

```sh
# Execute headlessly (exit 0/1/2)
talos run "<goal>" [--model <model>] [--budget <usd>] [--format json|text]

# Durable queue (same rows the desktop Agents page shows)
talos task list [--status <status>] [--format json]
talos task get <task_id> [--format json]
talos task cancel <task_id>

# Ledger integrity (both hash chains)
talos evidence verify [--format json]

# Policy selftest + chain verification
talos audit run [--format json]

# CLI-managed config (shared app_state → desktop agrees)
talos config get <key>
talos config set <key> <value>

# Provider registry (CLI mirror of Settings → Model providers)
talos provider list
talos provider add <name> [--set key=value ...]   # e.g. --set model=gpt-4o-mini
talos provider remove <name>

# MCP registry (CLI mirror of the MCP page)
talos mcp list
talos mcp add <name> [--set key=value ...]        # e.g. --set transport=stdio --set command=npx
talos mcp remove <name>

# Skill registry (same lifecycle as Memory page: quarantined → validated → published)
talos skill list [--status <status>]
talos skill validate <skill_id>                   # exit 2 on illegal transition
talos skill publish <skill_id>

# Environment check (toolchain, database, chain, encryption, onboarding flag)
talos doctor [--format json]

talos version
```

### `run` semantics

`talos run` enqueues a durable task, evaluates a read-only probe through
the broker, records the verdict + a run receipt as evidence, and completes.
Goals prefixed with `danger:` request an irreversible `SystemControl`
action instead — the broker answers `RequireCompensation`, the run is
failed, and the process exits **2**, proving headless mode cannot slip an
unsafe action past the gates:

```sh
talos --format json run "danger: wipe the disk"
# {"type":"task_started",...}
# {"type":"policy_evaluated","decision":"RequireCompensation",...}
# {"type":"task_failed","error":"policy denied",...}
$? → 2
```

Fidelity is reported honestly (`mock` for headless probes — never `live`,
never `verified`).

### `skill validate/publish` semantics

Promotions follow the same guarded lifecycle as the desktop
(`quarantined → validated → published`, no skips). An illegal transition
(e.g. publishing a quarantined skill) fails with exit code **2**, the same
"policy denied" channel as broker denials.

## Desktop IPC for CLI integration

- Both surfaces open the same file-backed database: the CLI resolves
  `TALOS_DATA_DIR` (or the platform default) and runs the desktop's exact
  open path — OS-keychain database key when available, loud plaintext
  fallback otherwise (never silent).
- If the desktop is running, the CLI reads/writes the **same live ledger**
  (`talos task list` and the Agents page agree; SQLite WAL allows
  concurrent readers/writers).
- If the desktop is not running, the CLI operates standalone on its own
  handle to the same file.
- Encrypted working-copy handshake assumes no competing sealer: prefer CLI
  writes while the desktop is closed, or isolate with `TALOS_DATA_DIR`.

## CI/CD recipe

```yaml
- name: Headless task
  run: |
    export TALOS_DATA_DIR=$RUNNER_TEMP/talos-data
    talos --format json run "regenerate API docs" --budget 2.50 > run.jsonl
    test $? -eq 0
    jq -e 'select(.type=="task_completed")' run.jsonl
    talos evidence verify --format json | jq -e '.event_chain_valid and .evidence_chain_valid'
```
