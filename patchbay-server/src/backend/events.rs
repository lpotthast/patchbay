use std::sync::{
    Arc, LazyLock, RwLock,
    atomic::{AtomicU64, Ordering},
};

use tokio::sync::broadcast;

use crate::{backend::storage::utc_now, shared::view_models::UiEvent};

const EVENT_BUFFER_SIZE: usize = 1024;

static EVENT_BUS: LazyLock<RwLock<Option<Arc<UiEventBus>>>> = LazyLock::new(|| RwLock::new(None));

#[derive(Debug)]
struct UiEventBus {
    sender: broadcast::Sender<UiEvent>,
    next_sequence: AtomicU64,
}

pub(crate) fn install() {
    let (sender, _) = broadcast::channel(EVENT_BUFFER_SIZE);
    let bus = UiEventBus {
        sender,
        next_sequence: AtomicU64::new(1),
    };
    *EVENT_BUS
        .write()
        .expect("Patchbay UI event bus lock is poisoned") = Some(Arc::new(bus));
}

pub(crate) fn subscribe() -> broadcast::Receiver<UiEvent> {
    event_bus().sender.subscribe()
}

fn publish(build: impl FnOnce(u64, String) -> UiEvent) {
    let Some(bus) = EVENT_BUS
        .read()
        .expect("Patchbay UI event bus lock is poisoned")
        .clone()
    else {
        return;
    };
    let sequence = bus.next_sequence.fetch_add(1, Ordering::Relaxed);
    let event = build(sequence, utc_now());
    let _ = bus.sender.send(event);
}

pub(crate) fn publish_project_list_changed() {
    publish(|sequence, timestamp| UiEvent::ProjectListChanged {
        sequence,
        timestamp,
    });
}

pub(crate) fn publish_project_changed(project: &str) {
    let project = project.to_owned();
    publish(|sequence, timestamp| UiEvent::ProjectChanged {
        sequence,
        timestamp,
        project,
    });
}

pub(crate) fn publish_work_item_changed(project: &str, item_id: i64) {
    let project = project.to_owned();
    publish(|sequence, timestamp| UiEvent::WorkItemChanged {
        sequence,
        timestamp,
        project,
        item_id,
    });
}

pub(crate) fn publish_comment_changed(project: &str, item_id: i64) {
    let project = project.to_owned();
    publish(|sequence, timestamp| UiEvent::CommentChanged {
        sequence,
        timestamp,
        project,
        item_id,
    });
}

pub(crate) fn publish_memory_changed(project: &str) {
    let project = project.to_owned();
    publish(|sequence, timestamp| UiEvent::MemoryChanged {
        sequence,
        timestamp,
        project,
    });
}

pub(crate) fn publish_swim_lane_changed(project: &str) {
    let project = project.to_owned();
    publish(|sequence, timestamp| UiEvent::SwimLaneChanged {
        sequence,
        timestamp,
        project,
    });
}

pub(crate) fn publish_agent_tool_changed() {
    publish(|sequence, timestamp| UiEvent::AgentToolChanged {
        sequence,
        timestamp,
    });
}

pub(crate) fn publish_automation_changed(project: &str) {
    let project = project.to_owned();
    publish(|sequence, timestamp| UiEvent::AutomationChanged {
        sequence,
        timestamp,
        project,
    });
}

pub(crate) fn publish_agent_run_changed(project: &str, run_id: i64, item_id: Option<i64>) {
    let project = project.to_owned();
    publish(|sequence, timestamp| UiEvent::AgentRunChanged {
        sequence,
        timestamp,
        project,
        run_id,
        item_id,
    });
}

pub(crate) fn publish_agent_output_changed(project: &str, run_id: i64, item_id: Option<i64>) {
    let project = project.to_owned();
    publish(|sequence, timestamp| UiEvent::AgentOutputChanged {
        sequence,
        timestamp,
        project,
        run_id,
        item_id,
    });
}

pub(crate) fn publish_codex_status_changed() {
    publish(|sequence, timestamp| UiEvent::CodexStatusChanged {
        sequence,
        timestamp,
    });
}

fn event_bus() -> Arc<UiEventBus> {
    let Some(bus) = EVENT_BUS
        .read()
        .expect("Patchbay UI event bus lock is poisoned")
        .clone()
    else {
        install();
        return EVENT_BUS
            .read()
            .expect("Patchbay UI event bus lock is poisoned")
            .as_ref()
            .expect("Patchbay UI event bus was just installed")
            .clone();
    };
    bus
}
