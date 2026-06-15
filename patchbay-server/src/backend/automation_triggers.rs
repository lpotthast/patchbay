use std::{collections::HashMap, str::FromStr, time::Duration as StdDuration};

use anyhow::{Context, Result, bail};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect,
};
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};
use tokio::sync::watch;

use crate::{
    backend::{
        automation::{self, AutomationTriggerOrigin, StartAutomation},
        automation_controller::AutomationController,
        entities::{
            automation_trigger::{
                self, AutomationTrigger, AutomationTriggerActiveModel, AutomationTriggerModel,
            },
            project, work_item_event,
        },
        process_sessions::ProcessSessionRegistry,
        projects,
        storage::{Store, utc_now},
    },
    shared::view_models::{
        AgentToolName, AutomationMode, AutomationTriggerView, TriggerKind, TriggerRunOutcome,
    },
};

#[derive(Clone, Debug)]
pub struct CreateAutomationTrigger {
    pub name: String,
    pub enabled: bool,
    pub trigger_kind: TriggerKind,
    pub schedule: Option<String>,
    pub mode: Option<AutomationMode>,
    pub tool_name: Option<AgentToolName>,
    pub prompt: String,
}

#[derive(Clone, Debug)]
pub struct UpdateAutomationTrigger {
    pub name: String,
    pub enabled: bool,
    pub trigger_kind: TriggerKind,
    pub schedule: Option<String>,
    pub prompt: String,
}

pub async fn list_triggers(
    store: &Store,
    project_name: &str,
) -> Result<Vec<AutomationTriggerView>> {
    let project_id = projects::project_id(store, project_name).await?;
    let triggers = AutomationTrigger::find()
        .filter(automation_trigger::Column::ProjectId.eq(project_id))
        .order_by_asc(automation_trigger::Column::Name)
        .all(store.db().as_ref())
        .await
        .context("failed to list automation triggers")?;
    triggers.into_iter().map(model_to_view).collect()
}

pub async fn create_trigger(
    store: &Store,
    project_name: &str,
    create: CreateAutomationTrigger,
) -> Result<AutomationTriggerView> {
    validate_create(&create)?;
    let project_id = projects::project_id(store, project_name).await?;
    let now = utc_now();
    let next_run_at = match create.trigger_kind {
        TriggerKind::Cron => Some(next_run_at(create.schedule.as_deref().unwrap())?),
        TriggerKind::WorkItemCreated => None,
    };
    let last_event_id = match create.trigger_kind {
        TriggerKind::Cron => None,
        TriggerKind::WorkItemCreated => latest_item_created_event_id(store, project_id).await?,
    };
    let mode = create
        .mode
        .unwrap_or_else(|| default_mode_for_kind(create.trigger_kind));
    let default_tool = crate::backend::projects::get_settings(store, project_name)
        .await?
        .default_agent_tool;
    let tool_name = create.tool_name.unwrap_or(default_tool);

    let trigger = AutomationTriggerActiveModel {
        project_id: Set(project_id),
        name: Set(create.name),
        enabled: Set(create.enabled),
        trigger_kind: Set(create.trigger_kind.as_storage().to_owned()),
        schedule: Set(create.schedule),
        mode: Set(mode.as_storage().to_owned()),
        tool_name: Set(tool_name.as_storage().to_owned()),
        prompt: Set(create.prompt),
        last_run_at: Set(None),
        next_run_at: Set(next_run_at),
        last_event_id: Set(last_event_id),
        created_at: Set(now.clone()),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(store.db().as_ref())
    .await
    .context("failed to create automation trigger")?;

    model_to_view(trigger)
}

pub async fn delete_trigger(store: &Store, project_name: &str, trigger_id: i64) -> Result<()> {
    let project_id = projects::project_id(store, project_name).await?;
    let trigger = AutomationTrigger::find_by_id(trigger_id)
        .filter(automation_trigger::Column::ProjectId.eq(project_id))
        .one(store.db().as_ref())
        .await
        .context("failed to load automation trigger")?
        .ok_or_else(|| anyhow::anyhow!("trigger {trigger_id} does not exist in this project"))?;
    AutomationTrigger::delete_by_id(trigger.id)
        .exec(store.db().as_ref())
        .await
        .context("failed to delete automation trigger")?;
    Ok(())
}

pub async fn update_trigger(
    store: &Store,
    project_name: &str,
    trigger_id: i64,
    update: UpdateAutomationTrigger,
) -> Result<AutomationTriggerView> {
    validate_update(&update)?;
    let project_id = projects::project_id(store, project_name).await?;
    let existing = AutomationTrigger::find_by_id(trigger_id)
        .filter(automation_trigger::Column::ProjectId.eq(project_id))
        .one(store.db().as_ref())
        .await
        .context("failed to load automation trigger")?
        .ok_or_else(|| anyhow::anyhow!("trigger {trigger_id} does not exist in this project"))?;
    let previous_kind = TriggerKind::from_str(&existing.trigger_kind)?;
    let now = utc_now();
    let next_run_at = match update.trigger_kind {
        TriggerKind::Cron => Some(next_run_at(update.schedule.as_deref().unwrap())?),
        TriggerKind::WorkItemCreated => None,
    };
    let last_event_id = match (previous_kind, update.trigger_kind) {
        (TriggerKind::WorkItemCreated, TriggerKind::WorkItemCreated) => existing.last_event_id,
        (TriggerKind::Cron, TriggerKind::WorkItemCreated) => {
            latest_item_created_event_id(store, project_id).await?
        }
        (_, TriggerKind::Cron) => None,
    };
    let default_tool = crate::backend::projects::get_settings(store, project_name)
        .await?
        .default_agent_tool;
    let mut active: AutomationTriggerActiveModel = existing.into();
    active.name = Set(update.name);
    active.enabled = Set(update.enabled);
    active.trigger_kind = Set(update.trigger_kind.as_storage().to_owned());
    active.schedule = Set(update.schedule);
    active.mode = Set(default_mode_for_kind(update.trigger_kind)
        .as_storage()
        .to_owned());
    active.tool_name = Set(default_tool.as_storage().to_owned());
    active.prompt = Set(update.prompt);
    active.next_run_at = Set(next_run_at);
    active.last_event_id = Set(last_event_id);
    active.updated_at = Set(now);

    let trigger = active
        .update(store.db().as_ref())
        .await
        .context("failed to update automation trigger")?;
    model_to_view(trigger)
}

pub async fn run_due_triggers(store: &Store) -> Result<Vec<TriggerRunOutcome>> {
    run_due_triggers_with_sessions(store, None).await
}

pub async fn run_due_triggers_with_sessions(
    store: &Store,
    sessions: Option<ProcessSessionRegistry>,
) -> Result<Vec<TriggerRunOutcome>> {
    run_due_triggers_with_sessions_for_projects(store, sessions, None, None).await
}

async fn run_due_triggers_with_sessions_for_projects(
    store: &Store,
    sessions: Option<ProcessSessionRegistry>,
    active_project_names: Option<&[String]>,
    project_cancellations: Option<&HashMap<String, watch::Receiver<bool>>>,
) -> Result<Vec<TriggerRunOutcome>> {
    let triggers = AutomationTrigger::find()
        .filter(automation_trigger::Column::Enabled.eq(true))
        .order_by_asc(automation_trigger::Column::Id)
        .all(store.db().as_ref())
        .await
        .context("failed to load enabled automation triggers")?;

    let mut outcomes = Vec::new();
    for trigger in triggers {
        let view = model_to_view(trigger.clone())?;
        let project_name = project_name_for_id(store, view.project_id).await?;
        if let Some(active_project_names) = active_project_names
            && !active_project_names.contains(&project_name)
        {
            continue;
        }
        match view.trigger_kind {
            TriggerKind::Cron => {
                if trigger_is_due(view.next_run_at.as_deref()) {
                    outcomes.push(
                        run_trigger_once(
                            store,
                            &project_name,
                            trigger,
                            None,
                            sessions.clone(),
                            project_cancellations
                                .and_then(|cancellations| cancellations.get(&project_name))
                                .cloned(),
                        )
                        .await,
                    );
                }
            }
            TriggerKind::WorkItemCreated => {
                let events =
                    new_item_created_events(store, view.project_id, view.last_event_id).await?;
                let mut last_event_id = view.last_event_id;
                for event in events {
                    last_event_id = Some(event.id);
                    outcomes.push(
                        run_trigger_once(
                            store,
                            &project_name,
                            trigger.clone(),
                            event.work_item_id,
                            sessions.clone(),
                            project_cancellations
                                .and_then(|cancellations| cancellations.get(&project_name))
                                .cloned(),
                        )
                        .await,
                    );
                }
                if last_event_id != view.last_event_id {
                    update_trigger_event_cursor(store, trigger, last_event_id).await?;
                }
            }
        }
    }
    Ok(outcomes)
}

pub fn spawn_scheduler_until(
    store: Store,
    sessions: Option<ProcessSessionRegistry>,
    controller: AutomationController,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(StdDuration::from_secs(15));
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    let project_cancellations = controller.project_cancellations().await;
                    if !project_cancellations.is_empty() {
                        let active_projects = project_cancellations
                            .keys()
                            .cloned()
                            .collect::<Vec<_>>();
                        if let Err(err) = run_due_triggers_with_sessions_for_projects(
                            &store,
                            sessions.clone(),
                            Some(&active_projects),
                            Some(&project_cancellations),
                        )
                        .await
                        {
                            eprintln!("automation trigger scheduler failed: {err:#}");
                        }
                    }
                    if let Err(err) = automation::recover_configured_stale_claims(&store).await {
                        eprintln!("stale claim recovery failed: {err:#}");
                    }
                }
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                }
            }
        }
    });
}

async fn run_trigger_once(
    store: &Store,
    project_name: &str,
    trigger: AutomationTriggerModel,
    work_item_id: Option<i64>,
    sessions: Option<ProcessSessionRegistry>,
    cancellation: Option<watch::Receiver<bool>>,
) -> TriggerRunOutcome {
    let view = match model_to_view(trigger.clone()) {
        Ok(view) => view,
        Err(err) => {
            return TriggerRunOutcome {
                trigger_id: trigger.id,
                trigger_name: trigger.name,
                work_item_id,
                run: None,
                error: Some(err.to_string()),
            };
        }
    };

    let result = automation::start_automation_with_sessions_until(
        store,
        project_name,
        StartAutomation {
            mode: view.mode,
            tool: None,
            work_item_id,
            extra_prompt: Some(view.prompt.clone()),
            trigger: Some(AutomationTriggerOrigin {
                trigger_id: view.id,
                trigger_name: view.name.clone(),
            }),
        },
        sessions,
        cancellation,
    )
    .await;

    let (run, error) = match result {
        Ok(run) => (Some(run), None),
        Err(err) => (None, Some(err.to_string())),
    };
    let _ = update_trigger_after_run(store, trigger).await;

    TriggerRunOutcome {
        trigger_id: view.id,
        trigger_name: view.name,
        work_item_id,
        run,
        error,
    }
}

async fn update_trigger_after_run(
    store: &Store,
    trigger: AutomationTriggerModel,
) -> Result<AutomationTriggerModel> {
    let view = model_to_view(trigger.clone())?;
    let now = utc_now();
    let next = if view.trigger_kind == TriggerKind::Cron {
        Some(next_run_at(view.schedule.as_deref().unwrap_or("@hourly"))?)
    } else {
        view.next_run_at
    };
    let mut active: AutomationTriggerActiveModel = trigger.into();
    active.last_run_at = Set(Some(now.clone()));
    active.next_run_at = Set(next);
    active.updated_at = Set(now);
    active
        .update(store.db().as_ref())
        .await
        .context("failed to update automation trigger after run")
}

async fn update_trigger_event_cursor(
    store: &Store,
    trigger: AutomationTriggerModel,
    last_event_id: Option<i64>,
) -> Result<AutomationTriggerModel> {
    let mut active: AutomationTriggerActiveModel = trigger.into();
    active.last_event_id = Set(last_event_id);
    active.updated_at = Set(utc_now());
    active
        .update(store.db().as_ref())
        .await
        .context("failed to update automation trigger event cursor")
}

async fn new_item_created_events(
    store: &Store,
    project_id: i64,
    last_event_id: Option<i64>,
) -> Result<Vec<work_item_event::Model>> {
    let mut query = work_item_event::Entity::find()
        .filter(work_item_event::Column::ProjectId.eq(project_id))
        .filter(work_item_event::Column::EventType.eq("item_created"))
        .order_by_asc(work_item_event::Column::Id);
    if let Some(last_event_id) = last_event_id {
        query = query.filter(work_item_event::Column::Id.gt(last_event_id));
    }
    query
        .all(store.db().as_ref())
        .await
        .context("failed to load item-created events")
}

pub(crate) async fn latest_item_created_event_id(
    store: &Store,
    project_id: i64,
) -> Result<Option<i64>> {
    let event = work_item_event::Entity::find()
        .filter(work_item_event::Column::ProjectId.eq(project_id))
        .filter(work_item_event::Column::EventType.eq("item_created"))
        .order_by_desc(work_item_event::Column::Id)
        .limit(1)
        .one(store.db().as_ref())
        .await
        .context("failed to load latest item-created event")?;
    Ok(event.map(|event| event.id))
}

async fn project_name_for_id(store: &Store, project_id: i64) -> Result<String> {
    let project = project::Entity::find_by_id(project_id)
        .one(store.db().as_ref())
        .await
        .context("failed to load trigger project")?
        .ok_or_else(|| anyhow::anyhow!("project {project_id} does not exist"))?;
    Ok(project.name)
}

fn validate_create(create: &CreateAutomationTrigger) -> Result<()> {
    validate_trigger_fields(
        &create.name,
        create.trigger_kind,
        create.schedule.as_deref(),
    )
}

fn validate_update(update: &UpdateAutomationTrigger) -> Result<()> {
    validate_trigger_fields(
        &update.name,
        update.trigger_kind,
        update.schedule.as_deref(),
    )
}

pub(crate) fn validate_trigger_fields(
    name: &str,
    trigger_kind: TriggerKind,
    schedule: Option<&str>,
) -> Result<()> {
    if name.trim().is_empty() {
        bail!("automation trigger name cannot be empty");
    }
    if trigger_kind == TriggerKind::Cron {
        let Some(schedule) = schedule else {
            bail!("cron trigger requires a schedule");
        };
        parse_schedule(schedule)?;
    }
    Ok(())
}

pub(crate) fn default_mode_for_kind(trigger_kind: TriggerKind) -> AutomationMode {
    match trigger_kind {
        TriggerKind::Cron => AutomationMode::Review,
        TriggerKind::WorkItemCreated => AutomationMode::Refine,
    }
}

fn trigger_is_due(next_run_at: Option<&str>) -> bool {
    let Some(next_run_at) = next_run_at else {
        return true;
    };
    let Ok(next) = OffsetDateTime::parse(next_run_at, &Rfc3339) else {
        return true;
    };
    next <= OffsetDateTime::now_utc()
}

pub(crate) fn next_run_at(schedule: &str) -> Result<String> {
    let interval = parse_schedule(schedule)?;
    (OffsetDateTime::now_utc() + interval)
        .format(&Rfc3339)
        .context("failed to format next trigger run time")
}

fn parse_schedule(schedule: &str) -> Result<Duration> {
    let value = schedule.trim();
    if value.eq_ignore_ascii_case("@hourly") {
        return Ok(Duration::hours(1));
    }
    if value.eq_ignore_ascii_case("@daily") {
        return Ok(Duration::days(1));
    }
    let value = value.strip_prefix("@every ").unwrap_or(value);
    let (number, suffix) = value.trim().split_at(
        value
            .trim()
            .find(|ch: char| !ch.is_ascii_digit())
            .unwrap_or(value.trim().len()),
    );
    if number.is_empty() {
        bail!("schedule must be @hourly, @daily, @every <duration>, or seconds");
    }
    let amount: i64 = number
        .parse()
        .with_context(|| format!("invalid schedule amount '{number}'"))?;
    if amount < 1 {
        bail!("schedule interval must be at least 1");
    }
    match suffix.trim().to_lowercase().as_str() {
        "" | "s" | "sec" | "secs" | "second" | "seconds" => Ok(Duration::seconds(amount)),
        "m" | "min" | "mins" | "minute" | "minutes" => Ok(Duration::minutes(amount)),
        "h" | "hr" | "hrs" | "hour" | "hours" => Ok(Duration::hours(amount)),
        "d" | "day" | "days" => Ok(Duration::days(amount)),
        other => bail!("unsupported schedule suffix '{other}'"),
    }
}

fn model_to_view(trigger: AutomationTriggerModel) -> Result<AutomationTriggerView> {
    Ok(AutomationTriggerView {
        id: trigger.id,
        project_id: trigger.project_id,
        name: trigger.name,
        enabled: trigger.enabled,
        trigger_kind: TriggerKind::from_str(&trigger.trigger_kind)?,
        schedule: trigger.schedule,
        mode: AutomationMode::from_str(&trigger.mode)?,
        tool_name: AgentToolName::from_str(&trigger.tool_name)?,
        prompt: trigger.prompt,
        last_run_at: trigger.last_run_at,
        next_run_at: trigger.next_run_at,
        last_event_id: trigger.last_event_id,
        created_at: trigger.created_at,
        updated_at: trigger.updated_at,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use tempfile::TempDir;

    use super::*;
    use crate::backend::{
        agent_tools::set_tool_path,
        items::{CreateWorkItem, create_item, get_item},
        projects::{CreateProject, create_project},
    };

    async fn test_store() -> (TempDir, Store) {
        let temp = TempDir::new().unwrap();
        let store = Store::open(temp.path().join("patchbay.sqlite3"))
            .await
            .unwrap();
        create_project(
            &store,
            CreateProject {
                name: "demo".to_owned(),
                display_name: None,
                path: temp.path().to_path_buf(),
                default_agent_model: None,
                system_prompt: None,
                memory: None,
            },
        )
        .await
        .unwrap();
        set_tool_path(&store, AgentToolName::Codex, PathBuf::from("/bin/echo"))
            .await
            .unwrap();
        (temp, store)
    }

    #[test]
    fn schedules_accept_every_notation() {
        assert!(parse_schedule("@every 15m").is_ok());
        assert!(parse_schedule("@hourly").is_ok());
        assert!(parse_schedule("0s").is_err());
    }

    #[tokio::test]
    async fn work_item_created_trigger_targets_new_item() {
        let (_temp, store) = test_store().await;
        create_trigger(
            &store,
            "demo",
            CreateAutomationTrigger {
                name: "refine-new-work".to_owned(),
                enabled: true,
                trigger_kind: TriggerKind::WorkItemCreated,
                schedule: None,
                mode: None,
                tool_name: None,
                prompt: "Refine this new work item.".to_owned(),
            },
        )
        .await
        .unwrap();
        let item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "New item".to_owned(),
                description: "Trigger should target this item".to_owned(),
                automation_claimable: true,
                agent_model_override: None,
                agent_reasoning_effort_override: None,
            },
        )
        .await
        .unwrap();

        let outcomes = run_due_triggers(&store).await.unwrap();
        let item = get_item(&store, "demo", item.id).await.unwrap();
        let triggers = list_triggers(&store, "demo").await.unwrap();
        let trigger = triggers
            .iter()
            .find(|trigger| trigger.name == "refine-new-work")
            .unwrap();

        assert_eq!(outcomes.len(), 1);

        let run = outcomes[0].run.as_ref().unwrap();
        let trigger_runs = automation::list_runs_for_trigger(&store, "demo", trigger.id, None)
            .await
            .unwrap();

        assert_eq!(outcomes[0].work_item_id, Some(item.id));
        assert!(outcomes[0].run.is_some());
        assert_eq!(run.trigger_id, Some(trigger.id));
        assert_eq!(run.trigger_name.as_deref(), Some("refine-new-work"));
        assert_eq!(trigger_runs.len(), 1);
        assert_eq!(trigger_runs[0].id, run.id);
        assert_eq!(item.claimed_by, None);
        assert_eq!(item.state.as_storage(), "open");
    }
}
