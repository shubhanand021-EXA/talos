# HEARTBEAT.md — Natural-Language Schedules for the Talos Scheduler

Talos reads `HEARTBEAT.md` (next to the app data dir, overridable for tests)
on daemon boot and fires each entry as a **system-initiated Supervisor task**
through the PolicyBroker — the same gates as any agent action, never bypassed.
Times are **UTC**; every expression maps deterministically to a five-field
cron string shown in Settings → Heartbeats.

## File shape

```md
## morning standup

- every morning at 9am
- summarize yesterday's ledger and post today's focus

## ledger sweep

- every 30 minutes
- summarize the ledger

## friday ship notes

- weekly on friday at 5pm
- draft release notes from completed tasks

## monthly billing

- monthly on the 15th at 10:00
- reconcile usage records against the plan
```

- `## <name>` headings start a task (max 32 tasks, 256 KiB file cap).
- Exactly **one** schedule line per task; remaining lines are the goal payload
  handed to the Supervisor DAG planner (keyword decomposition, no LLM needed).

## Supported expressions

| Say                                           | Fires                           | Cron                        |
| --------------------------------------------- | ------------------------------- | --------------------------- |
| `every 30 minutes` / `every 2 hours`          | interval                        | `*/30 * * * *`              |
| `every hour` / `hourly`                       | every 60 min                    | `0 */1 * * *`               |
| `every morning [at 9am]`                      | daily 09:00 default             | `0 9 * * *`                 |
| `every evening` / `every night`               | daily 18:00 / 23:00             | `0 18 * * *` / `0 23 * * *` |
| `every noon` / `every midnight` / `every day` | daily 12:00 / 00:00 / 09:00     | `0 12 * * *` …              |
| `daily at 09:00` / `daily at 9:30pm`          | daily                           | `30 21 * * *`               |
| `weekly on friday at 5pm`                     | weekly                          | `30 17 * * 5`               |
| `monthly on the 15th [at 10:00]`              | monthly (day 1, 09:00 defaults) | `0 10 15 * *`               |
| `on the 1st of the month`                     | monthly, 1st 09:00              | `0 9 1 * *`                 |
| `every month [at 9am]`                        | monthly, 1st                    | `0 9 1 * *`                 |

Times accept `HH:MM` (24h) or `9am` / `9:30pm` (12h). Ordinals accept
`1st 2nd 3rd Nth` (1–31); short months clamp to their last day (leap-aware:
Feb 29 exists only in leap years).

## What happens on fire

1. The daemon decomposes the goal into a DAG (`SupervisorAgent`, keyword
   path) and submits `[heartbeat] <name>` to the durable queue.
2. A policy gate evaluates the system action (`scheduler` agent). Goals naming
   mutating verbs (`delete`, `exec`, `deploy`, `publish`, …) become
   High/non-reversible and escalate to human review.
3. **Human review while away:** the task parks in `waiting_policy` and the
   daemon broadcasts `PolicyRequired` — the desktop shows it as a notification
   (Settings → Heartbeats, approval queue on return). Nothing executes.
4. Otherwise the task executes through the broker; `TaskStarted` /
   `TaskCompleted` broadcast to all clients and the schedule records
   `lastStatus` + `runCount`.

Paused schedules never fire (`toggle_schedule_active`); `trigger_schedule_now`
fires immediately even when paused. Missed periods never burst: the next
trigger skips ahead past `now`.
