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
