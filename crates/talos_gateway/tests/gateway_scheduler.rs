//! Scheduler trigger proof: due schedules submit system-initiated tasks
//! through the Supervisor + PolicyBroker; risky goals park in
//! `waiting_policy` with a `PolicyRequired` desktop notification; paused
//! schedules never fire.

use talos_gateway::{GatewayEventKind, GatewayState};

fn heartbeat_file(dir: &std::path::Path, body: &str) -> std::path::PathBuf {
    let path = dir.join("HEARTBEAT.md");
    std::fs::write(&path, body).expect("writes HEARTBEAT.md");
    path
}

fn scheduler_at(
    path: &std::path::Path,
    now: chrono::DateTime<chrono::Utc>,
) -> scheduler::Scheduler {
    scheduler::Scheduler::load_file(path, now)
}

#[tokio::test]
async fn scheduler_trigger_submits_system_task() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = heartbeat_file(
        dir.path(),
        "## probe\n- every 30 minutes\n- summarize the ledger\n",
    );
    // Anchor the scheduler an hour in the past so the 30-minute schedule is
    // already due at tick time.
    let past = chrono::Utc::now() - chrono::Duration::hours(1);
    let owned = scheduler_at(&path, past);
    assert_eq!(owned.len(), 1);
    let state = GatewayState::new_in_memory_with_scheduler(owned)
        .await
        .expect("state");
    let mut events = state.events.subscribe();
    state.run_scheduler_tick().await;
    // A system task reached the durable queue and completed via the broker.
    let tasks = evidence_store::list_tasks(&state.db, 10)
        .await
        .expect("lists");
    assert!(
        tasks.iter().any(|t| t.title == "[heartbeat] probe"),
        "system task submitted"
    );
    // Lifecycle broadcasts went out: TaskStarted then TaskCompleted.
    let mut saw_started = false;
    let mut saw_completed = false;
    for _ in 0..16 {
        let event = tokio::time::timeout(std::time::Duration::from_secs(5), events.recv())
            .await
            .expect("event arrives")
            .expect("channel open");
        match event.event {
            GatewayEventKind::TaskStarted => saw_started = true,
            GatewayEventKind::TaskCompleted => {
                saw_completed = true;
                break;
            }
            _ => {}
        }
    }
    assert!(saw_started && saw_completed, "lifecycle broadcast");
    // The schedule recorded its outcome.
    let status = state.list_schedules().await;
    assert_eq!(status.len(), 1);
    assert_eq!(status[0].run_count, 1);
    assert_eq!(status[0].last_status.as_deref(), Some("completed"));
}

#[tokio::test]
async fn risky_goal_parks_in_waiting_policy_with_notification() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = heartbeat_file(
        dir.path(),
        "## cleanup\n- every 30 minutes\n- delete stale caches\n",
    );
    let past = chrono::Utc::now() - chrono::Duration::hours(1);
    let owned = scheduler_at(&path, past);
    let state = GatewayState::new_in_memory_with_scheduler(owned)
        .await
        .expect("state");
    let mut events = state.events.subscribe();
    state.run_scheduler_tick().await;
    // High/non-reversible system action => parked for human review.
    let tasks = evidence_store::list_tasks(&state.db, 10)
        .await
        .expect("lists");
    let parked = tasks
        .iter()
        .find(|t| t.title == "[heartbeat] cleanup")
        .expect("system task");
    assert_eq!(parked.status, "waiting_policy");
    // Desktop notification broadcast carries the approval linkage.
    let mut notified = false;
    for _ in 0..16 {
        let event = tokio::time::timeout(std::time::Duration::from_secs(5), events.recv())
            .await
            .expect("event arrives")
            .expect("channel open");
        if event.event == GatewayEventKind::PolicyRequired {
            assert!(event.task_id.is_some());
            notified = true;
            break;
        }
    }
    assert!(notified, "PolicyRequired desktop notification");
    let status = state.list_schedules().await;
    assert_eq!(status[0].last_status.as_deref(), Some("waiting_policy"));
}

#[tokio::test]
async fn paused_schedule_does_not_submit() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = heartbeat_file(
        dir.path(),
        "## quiet\n- every 30 minutes\n- summarize the ledger\n",
    );
    let past = chrono::Utc::now() - chrono::Duration::hours(1);
    let mut owned = scheduler_at(&path, past);
    let id = owned.status()[0].id.clone();
    owned.set_active(&id, false, past);
    let state = GatewayState::new_in_memory_with_scheduler(owned)
        .await
        .expect("state");
    state.run_scheduler_tick().await;
    let tasks = evidence_store::list_tasks(&state.db, 10)
        .await
        .expect("lists");
    assert!(tasks.is_empty(), "paused schedule submits nothing");
}

#[tokio::test]
async fn trigger_now_fires_even_when_paused() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = heartbeat_file(
        dir.path(),
        "## manual\n- daily at 09:00\n- summarize the ledger\n",
    );
    let owned = scheduler_at(&path, chrono::Utc::now());
    let mut owned = owned;
    let id = owned.status()[0].id.clone();
    owned.set_active(&id, false, chrono::Utc::now());
    let state = GatewayState::new_in_memory_with_scheduler(owned)
        .await
        .expect("state");
    let view = state.trigger_schedule_now(&id).await.expect("manual fire");
    assert_eq!(view.status, "completed");
    let status = state.list_schedules().await;
    assert_eq!(status[0].run_count, 1);
}

#[tokio::test]
async fn destructive_goal_fails_closed_without_executing() {
    // Unattended deny: a wildcard-destructive heartbeat entry must reach a
    // terminal `failed` state through the broker — never executed, never
    // parked for a human who is not there to approve it.
    let dir = tempfile::tempdir().expect("temp dir");
    let path = heartbeat_file(
        dir.path(),
        "## wipe\n- every 30 minutes\n- rm -rf /tmp/cache\n",
    );
    let past = chrono::Utc::now() - chrono::Duration::hours(1);
    let owned = scheduler_at(&path, past);
    let state = GatewayState::new_in_memory_with_scheduler(owned)
        .await
        .expect("state");
    let mut events = state.events.subscribe();
    state.run_scheduler_tick().await;
    let tasks = evidence_store::list_tasks(&state.db, 10)
        .await
        .expect("lists");
    let task = tasks
        .iter()
        .find(|t| t.title == "[heartbeat] wipe")
        .expect("system task");
    assert_eq!(task.status, "failed", "destructive goal denied by broker");
    // No TaskStarted broadcast: the task never reached the worker.
    let mut saw_started = false;
    let mut saw_failed_completion = false;
    for _ in 0..16 {
        let event = tokio::time::timeout(std::time::Duration::from_secs(5), events.recv())
            .await
            .expect("event arrives")
            .expect("channel open");
        match event.event {
            GatewayEventKind::TaskStarted => saw_started = true,
            GatewayEventKind::TaskCompleted if event.task_id.as_deref() == Some(&task.id) => {
                saw_failed_completion = true;
                break;
            }
            _ => {}
        }
    }
    assert!(!saw_started, "denied task never starts executing");
    assert!(
        saw_failed_completion,
        "terminal failed completion broadcast"
    );
    let status = state.list_schedules().await;
    assert_eq!(status[0].run_count, 1);
    assert_eq!(status[0].last_status.as_deref(), Some("failed"));
}

#[tokio::test]
async fn scheduled_tasks_carry_the_unattended_budget_cap() {
    // Cost governor boundary: every unattended fire records the default
    // per-job cap on its task row, so the llm_gateway budget gate denies
    // over-budget traffic before any model call.
    let dir = tempfile::tempdir().expect("temp dir");
    let path = heartbeat_file(
        dir.path(),
        "## probe\n- every 30 minutes\n- summarize the ledger\n",
    );
    let past = chrono::Utc::now() - chrono::Duration::hours(1);
    let owned = scheduler_at(&path, past);
    let state = GatewayState::new_in_memory_with_scheduler(owned)
        .await
        .expect("state");
    state.run_scheduler_tick().await;
    let tasks = evidence_store::list_tasks(&state.db, 10)
        .await
        .expect("lists");
    let task = tasks
        .iter()
        .find(|t| t.title == "[heartbeat] probe")
        .expect("system task");
    let cap = evidence_store::get_task_budget(&state.db, &task.id)
        .await
        .expect("budget readable");
    assert_eq!(
        cap,
        Some(talos_gateway::DEFAULT_EXTERNAL_TASK_BUDGET_USD),
        "unattended default cap recorded"
    );
    assert!(cap.unwrap_or(0.0) > 0.0, "cap is a positive bound");
}
