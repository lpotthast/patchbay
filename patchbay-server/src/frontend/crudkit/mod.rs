use std::sync::Arc;

use crate::{
    frontend::{
        live_events::{event_scopes_named_project, reload_crudkit_on_live_event},
        rich_text::{
            normalize_tiptap_storage_value, rich_text_editor_html, rich_text_plain_text,
            tiptap_content_to_string,
        },
        types::{
            agent_tool::{
                AgentTool, AgentToolField, CreateAgentTool, CreateAgentToolField,
                CrudAgentToolResource, ReadAgentTool, ReadAgentToolField,
            },
            automation_trigger::{
                AutomationTrigger, AutomationTriggerField, CreateAutomationTrigger,
                CreateAutomationTriggerField, CrudAutomationTriggerResource, ReadAutomationTrigger,
                ReadAutomationTriggerField,
            },
            personality::{
                CreatePersonality, CreatePersonalityField, CrudPersonalityResource, Personality,
                PersonalityField, ReadPersonality, ReadPersonalityField,
            },
            project::{
                CreateProject, CreateProjectField, CrudProjectResource, Project as CrudProject,
                ProjectField, ReadProject, ReadProjectField,
            },
            swim_lane::{
                CreateSwimLane, CreateSwimLaneField, CrudSwimLaneResource, ReadSwimLane,
                ReadSwimLaneField, SwimLane, SwimLaneField,
            },
            work_item::{
                CreateWorkItem as CrudCreateWorkItem, CreateWorkItemField, CrudWorkItemResource,
                ReadWorkItem, ReadWorkItemField, WorkItem as CrudWorkItem, WorkItemField,
            },
            work_item_state::{
                CreateWorkItemState, CreateWorkItemStateField, CrudWorkItemStateResource,
                ReadWorkItemState, ReadWorkItemStateField, WorkItemState, WorkItemStateField,
            },
        },
        work_item_creation::CreateItemStateOption,
    },
    shared::view_models::{
        AgentReasoningEffort, CodexAgentModel, DEFAULT_STATE_LABEL, PersonalityView,
        ProjectLabelView, STATE_LABEL_KEY, UiEvent,
    },
};
use crudkit_leptos::crud_instance::CrudInstanceContext;
use crudkit_leptos::crudkit_core::{
    Value,
    condition::{Condition, ConditionClause, ConditionClauseValue, ConditionElement, Operator},
    id::{IdValue, SerializableId, SerializableIdEntry},
};
use crudkit_leptos::fields::{FieldRenderer, render_label};
use crudkit_leptos::{
    crud_instance_config::{
        CrudInstanceConfig, CrudNavigationConfig, FieldRendererRegistry, Header, ItemsPerPage,
        ModelHandler, PageNr,
    },
    crudkit_web::{
        HeaderOptions, Label, reqwest_executor::NewClientPerRequestExecutor,
        view::SerializableCrudView,
    },
    prelude::*,
};
use indexmap::indexmap;
use leptonic::components::prelude::{Icon, TiptapEditor};
use leptonic::prelude::icondata;
use leptos::prelude::*;
#[cfg(not(feature = "ssr"))]
use serde::Deserialize;

mod agent_tools;
mod automation_triggers;
mod personalities;
mod projects;
mod swim_lane_filter;
mod swim_lanes;
mod work_item_states;
mod work_items;

pub(crate) use agent_tools::agent_tools_panel;
pub(crate) use automation_triggers::{AutomationTableKind, automation_triggers_crudkit_instance};
pub(crate) use personalities::PersonalitiesPanel;
pub(crate) use projects::projects_panel;
pub(crate) use swim_lanes::SwimLanesPanel;
pub(crate) use work_item_states::WorkItemStatesPanel;
pub(crate) use work_items::{WorkItemsPanel, work_items_crudkit_config_for_view};

pub(crate) fn selected_trigger_id_from_context(context: CrudInstanceContext) -> Option<i64> {
    match context.view.get() {
        SerializableCrudView::Read(id) | SerializableCrudView::Edit(id) => serializable_i64_id(&id),
        SerializableCrudView::List | SerializableCrudView::Create => None,
    }
}

fn serializable_i64_id(id: &SerializableId) -> Option<i64> {
    id.entries().find_map(|entry| match &entry.value {
        IdValue::I64(value) => Some(*value),
        IdValue::I32(value) => Some(i64::from(*value)),
        IdValue::I16(value) => Some(i64::from(*value)),
        IdValue::I8(value) => Some(i64::from(*value)),
        _ => None,
    })
}

fn project_path_field_renderer<F: TypeErasedField>() -> FieldRenderer<F> {
    FieldRenderer::new(
        move |_signals, _field: F, field_mode, field_options, value, value_changed| {
            let current =
                Signal::derive(move || value.value.get().as_string().cloned().unwrap_or_default());

            match field_mode {
                FieldMode::Display => view! { {move || current.get()} }.into_any(),
                FieldMode::Readable | FieldMode::Editable => {
                    let disabled = field_mode != FieldMode::Editable || field_options.disabled;
                    view! {
                        {render_label(field_options.label.clone())}
                        <div class="project-path-field">
                            <div class="project-path-input-row">
                                <input
                                    type="text"
                                    class="crud-input-field project-path-text"
                                    prop:value=move || current.get()
                                    disabled=disabled
                                    placeholder="~/dev/project"
                                    on:input=move |event| {
                                        value_changed.run(Ok(Value::String(event_target_value(&event))));
                                    }
                                />
                                <button
                                    type="button"
                                    class="project-path-picker"
                                    disabled=disabled
                                    on:click=move |_| {
                                        let value_changed = value_changed;
                                        leptos::task::spawn_local(async move {
                                            if let Some(path) = pick_project_folder_path().await {
                                                value_changed.run(Ok(Value::String(path)));
                                            }
                                        });
                                    }
                                >
                                    "Choose folder"
                                </button>
                            </div>
                        </div>
                    }
                    .into_any()
                }
            }
        },
    )
}

fn multiline_text_field_renderer<F: TypeErasedField>(
    placeholder: &'static str,
) -> FieldRenderer<F> {
    FieldRenderer::new(
        move |_signals, _field: F, field_mode, field_options, value, value_changed| {
            let current =
                Signal::derive(move || value.value.get().as_string().cloned().unwrap_or_default());

            match field_mode {
                FieldMode::Display => view! { {move || current.get()} }.into_any(),
                FieldMode::Readable | FieldMode::Editable => {
                    let disabled = field_mode != FieldMode::Editable || field_options.disabled;
                    view! {
                        {render_label(field_options.label.clone())}
                        <textarea
                            class="crud-input-field"
                            prop:value=move || current.get()
                            disabled=disabled
                            placeholder=placeholder
                            on:input=move |event| {
                                value_changed.run(Ok(Value::String(event_target_value(&event))));
                            }
                        />
                    }
                    .into_any()
                }
            }
        },
    )
}

fn rich_text_field_renderer<F: TypeErasedField>(label: &'static str) -> FieldRenderer<F> {
    FieldRenderer::new(
        move |_signals, field: F, field_mode, field_options, value, value_changed| {
            let field_name = field.name().into_owned();
            let field_name_attr = field_name.clone();
            let field_name_input = field_name.clone();
            let current =
                Signal::derive(move || value.value.get().as_string().cloned().unwrap_or_default());

            match field_mode {
                FieldMode::Display => {
                    view! { {move || rich_text_plain_text(&current.get())} }.into_any()
                }
                FieldMode::Readable | FieldMode::Editable => {
                    let disabled = field_mode != FieldMode::Editable || field_options.disabled;
                    let seen_editor_update = RwSignal::new(false);
                    let editor_value =
                        Signal::derive(move || rich_text_editor_html(&current.get()));
                    view! {
                        {render_label(field_options.label.clone().or_else(|| Some(Label::new(label))))}
                        <div
                            class="rich-text-field crud-rich-text-field"
                            data-rich-text-field=field_name_attr
                            on:click=|event| {
                                // TipTap may render anchors/buttons; editor clicks should not activate page-level defaults.
                                event.prevent_default();
                            }
                        >
                            <input
                                type="hidden"
                                class="rich-text-input crud-input-field"
                                name=field_name_input
                                value=move || current.get()
                                on:input=move |event| {
                                    value_changed.run(Ok(Value::String(event_target_value(&event))));
                                }
                            />
                            <TiptapEditor
                                value=editor_value
                                disabled=Signal::derive(move || disabled)
                                set_value=move |content| {
                                    let current_value = current.get_untracked();
                                    let next_value = normalize_tiptap_storage_value(tiptap_content_to_string(content));
                                    let first_editor_update = !seen_editor_update.get_untracked();
                                    seen_editor_update.set(true);
                                    if first_editor_update
                                        && rich_text_plain_text(&next_value) == rich_text_plain_text(&current_value)
                                    {
                                        return;
                                    }
                                    if next_value != current_value {
                                        value_changed.run(Ok(Value::String(next_value)));
                                    }
                                }
                            />
                        </div>
                    }
                    .into_any()
                }
            }
        },
    )
}

#[cfg(not(feature = "ssr"))]
#[derive(Deserialize)]
struct PickFolderResponse {
    path: Option<String>,
}

#[cfg(not(feature = "ssr"))]
async fn pick_project_folder_path() -> Option<String> {
    let response = gloo_net::http::Request::post("/system/pick-folder")
        .send()
        .await
        .ok()?;
    if !response.ok() {
        return None;
    }
    response
        .json::<PickFolderResponse>()
        .await
        .ok()?
        .path
        .map(|path| path.trim().to_owned())
        .filter(|path| !path.is_empty())
}

#[cfg(feature = "ssr")]
async fn pick_project_folder_path() -> Option<String> {
    None
}

fn project_path_status_renderer<F: TypeErasedField>() -> FieldRenderer<F> {
    FieldRenderer::new(
        move |_signals, _field: F, _field_mode, _field_options, value, _value_changed| {
            let exists = Signal::derive(move || value.value.get().as_bool().unwrap_or(false));
            view! {
                <span class=move || {
                    if exists.get() {
                        "path-status path-status-ok"
                    } else {
                        "path-status path-status-missing"
                    }
                }>
                    {move || if exists.get() { "Exists" } else { "Missing" }}
                </span>
            }
        },
    )
}

fn agent_model_field_renderer<F: TypeErasedField>(
    empty_label: Option<&'static str>,
) -> FieldRenderer<F> {
    FieldRenderer::new(
        move |_signals, _field: F, field_mode, field_options, value, value_changed| {
            let current =
                Signal::derive(move || value.value.get().as_string().cloned().unwrap_or_default());

            match field_mode {
                FieldMode::Display => view! {
                    <span class=move || agent_model_class(&current.get())>
                        {move || agent_model_label(&current.get(), empty_label.unwrap_or("default"))}
                    </span>
                }
                .into_any(),
                FieldMode::Readable | FieldMode::Editable => {
                    let disabled = field_mode != FieldMode::Editable || field_options.disabled;
                    let options = CodexAgentModel::all()
                        .iter()
                        .map(|model| {
                            let value = model.as_storage();
                            view! {
                                <option value=value>{value}</option>
                            }
                        })
                        .collect::<Vec<_>>();
                    let stale_option = move || {
                        let current = current.get();
                        (!current.is_empty() && !CodexAgentModel::is_available_model(&current))
                            .then(|| {
                                let label = format!("{current} (unavailable)");
                                view! { <option value=current>{label}</option> }
                            })
                    };
                    let stale_warning = move || {
                        let current = current.get();
                        (!current.is_empty() && !CodexAgentModel::is_available_model(&current))
                            .then(|| {
                                view! {
                                    <p class="agent-model-warning">
                                        "Saved model is not available in this Codex install."
                                    </p>
                                }
                            })
                    };
                    let empty_option = empty_label.map(|empty_label| {
                        view! { <option value="">{empty_label}</option> }
                    });
                    view! {
                        {render_label(field_options.label.clone())}
                        <select
                            class="crud-input-field agent-model-select"
                            prop:value=move || current.get()
                            disabled=disabled
                            on:change=move |event| {
                                let selected = event_target_value(&event);
                                if selected.trim().is_empty() {
                                    value_changed.run(Ok(Value::Null));
                                } else {
                                    value_changed.run(Ok(Value::String(selected)));
                                }
                            }
                        >
                            {empty_option}
                            {stale_option}
                            {options}
                        </select>
                        {stale_warning}
                    }
                    .into_any()
                }
            }
        },
    )
}

fn agent_model_label(value: &str, empty_label: &str) -> String {
    if value.is_empty() {
        empty_label.to_owned()
    } else if CodexAgentModel::is_available_model(value) {
        value.to_owned()
    } else {
        format!("{value} (unavailable)")
    }
}

fn agent_model_class(value: &str) -> &'static str {
    if value.is_empty() {
        "agent-model-value agent-model-default"
    } else if CodexAgentModel::is_available_model(value) {
        "agent-model-value"
    } else {
        "agent-model-value agent-model-stale"
    }
}

fn agent_reasoning_field_renderer<F: TypeErasedField>(
    empty_label: Option<&'static str>,
) -> FieldRenderer<F> {
    FieldRenderer::new(
        move |_signals, _field: F, field_mode, field_options, value, value_changed| {
            let current =
                Signal::derive(move || value.value.get().as_string().cloned().unwrap_or_default());

            match field_mode {
                FieldMode::Display => view! {
                    {move || {
                        let current = current.get();
                        if current.is_empty() {
                            empty_label.unwrap_or("default").to_owned()
                        } else {
                            current
                        }
                    }}
                }
                .into_any(),
                FieldMode::Readable | FieldMode::Editable => {
                    let disabled = field_mode != FieldMode::Editable || field_options.disabled;
                    let options = AgentReasoningEffort::all()
                        .into_iter()
                        .map(|effort| {
                            let value = effort.as_storage();
                            view! {
                                <option value=value>{effort.to_string()}</option>
                            }
                        })
                        .collect::<Vec<_>>();
                    let empty_option = empty_label.map(|empty_label| {
                        view! { <option value="">{empty_label}</option> }
                    });
                    view! {
                        {render_label(field_options.label.clone())}
                        <select
                            class="crud-input-field agent-reasoning-select"
                            prop:value=move || current.get()
                            disabled=disabled
                            on:change=move |event| {
                                let selected = event_target_value(&event);
                                if selected.trim().is_empty() {
                                    value_changed.run(Ok(Value::Null));
                                } else {
                                    value_changed.run(Ok(Value::String(selected)));
                                }
                            }
                        >
                            {empty_option}
                            {options}
                        </select>
                    }
                    .into_any()
                }
            }
        },
    )
}

fn activation_field_renderer<F: TypeErasedField>(
    choices: &'static [(&'static str, &'static str)],
) -> FieldRenderer<F> {
    select_field_renderer(choices, false)
}

fn select_field_renderer<F: TypeErasedField>(
    choices: &'static [(&'static str, &'static str)],
    nullable: bool,
) -> FieldRenderer<F> {
    FieldRenderer::new(
        move |_signals, _field: F, field_mode, field_options, value, value_changed| {
            let current =
                Signal::derive(move || value.value.get().as_string().cloned().unwrap_or_default());

            match field_mode {
                FieldMode::Display => view! {
                    {move || {
                        let current = current.get();
                        choices
                            .iter()
                            .find(|(value, _)| *value == current)
                            .map(|(_, label)| (*label).to_owned())
                            .unwrap_or(current)
                    }}
                }
                .into_any(),
                FieldMode::Readable | FieldMode::Editable => {
                    let disabled = field_mode != FieldMode::Editable || field_options.disabled;
                    let options = choices
                        .iter()
                        .map(|(value, label)| {
                            view! {
                                <option value=*value>{*label}</option>
                            }
                        })
                        .collect::<Vec<_>>();
                    let empty_option = nullable.then(|| {
                        view! { <option value="">"default"</option> }
                    });
                    view! {
                        {render_label(field_options.label.clone())}
                        <select
                            class="crud-input-field"
                            prop:value=move || current.get()
                            disabled=disabled
                            on:change=move |event| {
                                let selected = event_target_value(&event);
                                if nullable && selected.trim().is_empty() {
                                    value_changed.run(Ok(Value::Null));
                                } else {
                                    value_changed.run(Ok(Value::String(selected)));
                                }
                            }
                        >
                            {empty_option}
                            {options}
                        </select>
                    }
                    .into_any()
                }
            }
        },
    )
}

fn personality_field_renderer<F: TypeErasedField>(
    personalities: Vec<PersonalityView>,
) -> FieldRenderer<F> {
    FieldRenderer::new(
        move |_signals, _field: F, field_mode, field_options, value, value_changed| {
            let personalities = personalities.clone();
            let current = Signal::derive(move || value_to_optional_i64(&value.value.get()));

            match field_mode {
                FieldMode::Display => view! {
                    {move || {
                        let current = current.get();
                        current
                            .and_then(|id| {
                                personalities
                                    .iter()
                                    .find(|personality| personality.id == id)
                                    .map(|personality| personality.name.clone())
                            })
                            .unwrap_or_else(|| "Default".to_owned())
                    }}
                }
                .into_any(),
                FieldMode::Readable | FieldMode::Editable => {
                    let disabled = field_mode != FieldMode::Editable || field_options.disabled;
                    let options = personalities
                        .iter()
                        .map(|personality| {
                            let id = personality.id.to_string();
                            let name = personality.name.clone();
                            view! { <option value=id>{name}</option> }
                        })
                        .collect::<Vec<_>>();
                    view! {
                        {render_label(field_options.label.clone())}
                        <select
                            class="crud-input-field"
                            prop:value=move || {
                                current
                                    .get()
                                    .map(|id| id.to_string())
                                    .unwrap_or_default()
                            }
                            disabled=disabled
                            on:change=move |event| {
                                let selected = event_target_value(&event);
                                match selected.parse::<i64>() {
                                    Ok(id) => value_changed.run(Ok(Value::I64(id))),
                                    Err(_) => value_changed.run(Ok(Value::Null)),
                                }
                            }
                        >
                            {options}
                        </select>
                    }
                    .into_any()
                }
            }
        },
    )
}

fn value_to_optional_i64(value: &Value) -> Option<i64> {
    match value {
        Value::I64(value) => Some(*value),
        Value::I32(value) => Some(i64::from(*value)),
        Value::I16(value) => Some(i64::from(*value)),
        Value::I8(value) => Some(i64::from(*value)),
        Value::U64(value) => i64::try_from(*value).ok(),
        Value::U32(value) => Some(i64::from(*value)),
        Value::U16(value) => Some(i64::from(*value)),
        Value::U8(value) => Some(i64::from(*value)),
        Value::String(value) => value.parse::<i64>().ok(),
        Value::Null | Value::Void(()) => None,
        _ => None,
    }
}

fn swim_lane_order_field_renderer<F: TypeErasedField>() -> FieldRenderer<F> {
    select_field_renderer(
        &[
            ("updated_desc", "Updated newest first"),
            ("updated_asc", "Updated oldest first"),
            ("created_desc", "Created newest first"),
            ("created_asc", "Created oldest first"),
            ("id_desc", "ID descending"),
            ("id_asc", "ID ascending"),
            ("title_asc", "Title A-Z"),
            ("title_desc", "Title Z-A"),
        ],
        false,
    )
}

pub(crate) fn crudkit_i64_id(id: i64) -> SerializableId {
    SerializableId(vec![SerializableIdEntry {
        field_name: "id".to_owned(),
        value: IdValue::I64(id),
    }])
}

fn project_id_condition(project_id: i64) -> Condition {
    Condition::All(vec![ConditionElement::Clause(ConditionClause {
        column_name: "project_id".to_owned(),
        operator: Operator::Equal,
        value: ConditionClauseValue::I64(project_id),
    })])
}

fn automation_effect_condition(project_id: i64, effect: &str) -> Condition {
    Condition::All(vec![
        ConditionElement::Clause(ConditionClause {
            column_name: "project_id".to_owned(),
            operator: Operator::Equal,
            value: ConditionClauseValue::I64(project_id),
        }),
        ConditionElement::Clause(ConditionClause {
            column_name: "effect".to_owned(),
            operator: Operator::Equal,
            value: ConditionClauseValue::String(effect.to_owned()),
        }),
    ])
}
