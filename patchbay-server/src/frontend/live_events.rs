use crate::shared::view_models::UiEvent;
#[cfg(not(feature = "ssr"))]
use codee::string::FromToStringCodec;
use crudkit_leptos::crud_instance::CrudInstanceContext;
use leptos::prelude::*;
#[cfg(not(feature = "ssr"))]
use leptos_use::{
    ReconnectLimit, UseWebSocketOptions, UseWebSocketReturn, use_websocket_with_options,
};

#[derive(Clone, Copy)]
struct LiveEventContext {
    latest_event: ReadSignal<Option<UiEvent>>,
}

#[component]
pub(crate) fn LiveEventsProvider() -> impl IntoView {
    let (latest_event, set_latest_event) = signal(None::<UiEvent>);
    provide_context(LiveEventContext { latest_event });
    #[cfg(feature = "ssr")]
    let _ = set_latest_event;

    #[cfg(not(feature = "ssr"))]
    {
        let UseWebSocketReturn { message, .. } =
            use_websocket_with_options::<String, String, FromToStringCodec, _, _>(
                "/api/events/ws",
                UseWebSocketOptions::default()
                    .reconnect_limit(ReconnectLimit::Infinite)
                    .reconnect_interval(1_000),
            );
        Effect::new(move |_| {
            if let Some(raw) = message.get()
                && let Ok(event) = serde_json::from_str::<UiEvent>(&raw)
            {
                set_latest_event.set(Some(event));
            }
        });
    }
}

pub(crate) fn refetch_on_live_event<T>(
    resource: LocalResource<T>,
    should_refetch: impl Fn(&UiEvent) -> bool + 'static,
) where
    T: 'static,
{
    if let Some(context) = use_context::<LiveEventContext>() {
        Effect::new(move |_| {
            if let Some(event) = context.latest_event.get()
                && should_refetch(&event)
            {
                resource.refetch();
            }
        });
    }
}

pub(crate) fn reload_crudkit_on_live_event(
    context: ReadSignal<Option<CrudInstanceContext>>,
    should_reload: impl Fn(&UiEvent) -> bool + 'static,
) {
    if let Some(live) = use_context::<LiveEventContext>() {
        Effect::new(move |_| {
            if let Some(event) = live.latest_event.get()
                && should_reload(&event)
                && let Some(context) = context.get()
            {
                context.reload();
            }
        });
    }
}

pub(crate) fn event_scopes_named_project(event: &UiEvent, project: Option<&str>) -> bool {
    match (project, event_project(event)) {
        (Some(expected), Some(actual)) => expected == actual,
        (Some(_), None) => true,
        (None, _) => true,
    }
}

fn event_project(event: &UiEvent) -> Option<&str> {
    match event {
        UiEvent::ProjectChanged { project, .. }
        | UiEvent::SystemPromptChanged { project, .. }
        | UiEvent::WorkItemChanged { project, .. }
        | UiEvent::CommentChanged { project, .. }
        | UiEvent::MemoryChanged { project, .. }
        | UiEvent::SwimLaneChanged { project, .. }
        | UiEvent::WorkItemStateChanged { project, .. }
        | UiEvent::AutomationChanged { project, .. }
        | UiEvent::AgentRunChanged { project, .. }
        | UiEvent::AgentOutputChanged { project, .. } => Some(project),
        UiEvent::ProjectListChanged { .. }
        | UiEvent::AgentToolChanged { .. }
        | UiEvent::CodexStatusChanged { .. } => None,
    }
}
