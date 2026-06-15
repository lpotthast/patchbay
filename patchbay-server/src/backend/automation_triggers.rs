use std::{collections::HashMap, str::FromStr, time::Duration as StdDuration};

use crudkit_core::condition::Condition;
use rootcause::{Result, prelude::*};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter,
    QueryOrder, QuerySelect,
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
        events,
        items::{self, CreateWorkItem},
        process_sessions::ProcessSessionRegistry,
        projects,
        storage::{Store, utc_now},
    },
    shared::view_models::{
        AgentToolName, AutomationActivation, AutomationEffect, AutomationMode,
        AutomationTriggerView, TriggerRunOutcome, default_automation_work_item_selector,
    },
};

const DEFAULT_WORK_ITEM_AUTOMATION_NAME: &str = "Claim open work";
const DEFAULT_WORK_ITEM_AUTOMATION_SCHEDULE: &str = "@every 15s";
const SCHEDULER_TICK_SECONDS: u64 = 1;
const MAINTENANCE_TICK_SECONDS: u64 = 15;
const PRIORITY_SCORE_SECONDS: i64 = 300;
const EVALUATION_COUNT_SCORE_SECONDS: i64 = 300;
const NEVER_RUN_SCORE_SECONDS: i64 = 24 * 60 * 60;

#[derive(Clone, Debug)]
pub struct CreateAutomationTrigger {
    pub name: String,
    pub enabled: bool,
    pub activation: AutomationActivation,
    pub effect: AutomationEffect,
    pub schedule: String,
    pub mode: Option<AutomationMode>,
    pub tool_name: Option<AgentToolName>,
    pub prompt: String,
    pub work_item_selector: Option<Condition>,
    pub priority: i64,
}

#[derive(Clone, Debug)]
pub struct UpdateAutomationTrigger {
    pub name: String,
    pub enabled: bool,
    pub activation: AutomationActivation,
    pub effect: AutomationEffect,
    pub schedule: String,
    pub prompt: String,
    pub work_item_selector: Option<Condition>,
    pub priority: Option<i64>,
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
    let project_id = projects::project_id(store, project_name).await?;
    let now = utc_now();
    let schedule = normalize_schedule(create.schedule)?;
    let mode = create
        .mode
        .unwrap_or_else(|| default_mode_for_activation(create.activation));
    let work_item_selector = selector_for_activation(create.activation, create.work_item_selector)?;
    validate_trigger_configuration(
        &create.name,
        create.activation,
        create.effect,
        &schedule,
        mode,
        work_item_selector.as_ref(),
        &create.prompt,
    )?;
    let next_evaluation_at = match create.activation {
        AutomationActivation::Manual => None,
        AutomationActivation::WorkItem => None,
        AutomationActivation::Cron => Some(next_evaluation_at(&schedule)?),
        AutomationActivation::WorkItemCreated => None,
    };
    let last_event_id = match create.activation {
        AutomationActivation::Manual
        | AutomationActivation::WorkItem
        | AutomationActivation::Cron => None,
        AutomationActivation::WorkItemCreated => {
            latest_item_created_event_id(store, project_id).await?
        }
    };
    let default_tool = crate::backend::projects::get_settings(store, project_name)
        .await?
        .default_agent_tool;
    let tool_name = create.tool_name.unwrap_or(default_tool);

    let trigger = AutomationTriggerActiveModel {
        project_id: Set(project_id),
        name: Set(create.name),
        enabled: Set(create.enabled),
        activation: Set(create.activation.as_storage().to_owned()),
        effect: Set(create.effect.as_storage().to_owned()),
        schedule: Set(schedule),
        mode: Set(mode.as_storage().to_owned()),
        tool_name: Set(tool_name.as_storage().to_owned()),
        prompt: Set(create.prompt),
        work_item_selector: Set(selector_to_storage(work_item_selector.as_ref())?),
        priority: Set(create.priority),
        evaluation_count: Set(0),
        pending_evaluation_count: Set(0),
        last_evaluation_queued_at: Set(None),
        last_evaluated_at: Set(None),
        next_evaluation_at: Set(next_evaluation_at),
        last_event_id: Set(last_event_id),
        created_at: Set(now.clone()),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(store.db().as_ref())
    .await
    .context("failed to create automation trigger")?;

    events::publish_automation_changed(project_name);
    model_to_view(trigger)
}

pub async fn delete_trigger(store: &Store, project_name: &str, trigger_id: i64) -> Result<()> {
    let project_id = projects::project_id(store, project_name).await?;
    let trigger = AutomationTrigger::find_by_id(trigger_id)
        .filter(automation_trigger::Column::ProjectId.eq(project_id))
        .one(store.db().as_ref())
        .await
        .context("failed to load automation trigger")?
        .ok_or_else(|| report!("trigger {trigger_id} does not exist in this project"))?;
    AutomationTrigger::delete_by_id(trigger.id)
        .exec(store.db().as_ref())
        .await
        .context("failed to delete automation trigger")?;
    events::publish_automation_changed(project_name);
    Ok(())
}

pub async fn update_trigger(
    store: &Store,
    project_name: &str,
    trigger_id: i64,
    update: UpdateAutomationTrigger,
) -> Result<AutomationTriggerView> {
    let project_id = projects::project_id(store, project_name).await?;
    let existing = AutomationTrigger::find_by_id(trigger_id)
        .filter(automation_trigger::Column::ProjectId.eq(project_id))
        .one(store.db().as_ref())
        .await
        .context("failed to load automation trigger")?
        .ok_or_else(|| report!("trigger {trigger_id} does not exist in this project"))?;
    let previous_kind = AutomationActivation::from_str(&existing.activation)?;
    let schedule = normalize_schedule(update.schedule)?;
    let mode = default_mode_for_activation(update.activation);
    let work_item_selector = selector_for_activation(update.activation, update.work_item_selector)?;
    validate_trigger_configuration(
        &update.name,
        update.activation,
        update.effect,
        &schedule,
        mode,
        work_item_selector.as_ref(),
        &update.prompt,
    )?;
    let now = utc_now();
    let next_evaluation_at = match update.activation {
        AutomationActivation::Manual => None,
        AutomationActivation::WorkItem => None,
        AutomationActivation::Cron => Some(next_evaluation_at(&schedule)?),
        AutomationActivation::WorkItemCreated => None,
    };
    let last_event_id = match (previous_kind, update.activation) {
        (AutomationActivation::WorkItemCreated, AutomationActivation::WorkItemCreated) => {
            existing.last_event_id
        }
        (_, AutomationActivation::WorkItemCreated) => {
            latest_item_created_event_id(store, project_id).await?
        }
        (
            _,
            AutomationActivation::Manual
            | AutomationActivation::WorkItem
            | AutomationActivation::Cron,
        ) => None,
    };
    let default_tool = crate::backend::projects::get_settings(store, project_name)
        .await?
        .default_agent_tool;
    let mut active: AutomationTriggerActiveModel = existing.into();
    active.name = Set(update.name);
    active.enabled = Set(update.enabled);
    active.activation = Set(update.activation.as_storage().to_owned());
    active.effect = Set(update.effect.as_storage().to_owned());
    active.schedule = Set(schedule);
    active.mode = Set(mode.as_storage().to_owned());
    active.tool_name = Set(default_tool.as_storage().to_owned());
    active.prompt = Set(update.prompt);
    active.work_item_selector = Set(selector_to_storage(work_item_selector.as_ref())?);
    if let Some(priority) = update.priority {
        active.priority = Set(priority);
    }
    active.next_evaluation_at = Set(next_evaluation_at);
    active.last_event_id = Set(last_event_id);
    active.updated_at = Set(now);

    let trigger = active
        .update(store.db().as_ref())
        .await
        .context("failed to update automation trigger")?;
    events::publish_automation_changed(project_name);
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
    let mut outcomes =
        run_queued_evaluations(store, sessions.clone(), project_cancellations).await?;
    let triggers = AutomationTrigger::find()
        .filter(automation_trigger::Column::Enabled.eq(true))
        .order_by_asc(automation_trigger::Column::Id)
        .all(store.db().as_ref())
        .await
        .context("failed to load enabled automation triggers")?;

    for trigger in triggers {
        let view = model_to_view(trigger.clone())?;
        if matches!(
            view.activation,
            AutomationActivation::Manual | AutomationActivation::WorkItem
        ) {
            continue;
        }
        let project_name = project_name_for_id(store, view.project_id).await?;
        if let Some(active_project_names) = active_project_names
            && !active_project_names.contains(&project_name)
        {
            continue;
        }
        match view.activation {
            AutomationActivation::Manual => {}
            AutomationActivation::WorkItem => {}
            AutomationActivation::Cron => {
                if trigger_is_due(view.next_evaluation_at.as_deref())
                    && let Some(outcome) = evaluate_trigger_once(
                        store,
                        &project_name,
                        trigger,
                        None,
                        sessions.clone(),
                        project_cancellations
                            .and_then(|cancellations| cancellations.get(&project_name))
                            .cloned(),
                    )
                    .await
                {
                    outcomes.push(outcome);
                }
            }
            AutomationActivation::WorkItemCreated => {
                let events =
                    new_item_created_events(store, view.project_id, view.last_event_id).await?;
                let mut last_event_id = view.last_event_id;
                for event in events {
                    last_event_id = Some(event.id);
                    if let Some(outcome) = evaluate_trigger_once(
                        store,
                        &project_name,
                        trigger.clone(),
                        event.work_item_id,
                        sessions.clone(),
                        project_cancellations
                            .and_then(|cancellations| cancellations.get(&project_name))
                            .cloned(),
                    )
                    .await
                    {
                        outcomes.push(outcome);
                    }
                }
                if last_event_id != view.last_event_id {
                    update_trigger_event_cursor(store, trigger, last_event_id).await?;
                }
            }
        }
    }
    if let Some(active_project_names) = active_project_names {
        for project_name in active_project_names {
            if let Some(outcome) = run_next_work_item_automation_for_project(
                store,
                project_name,
                sessions.clone(),
                project_cancellations
                    .and_then(|cancellations| cancellations.get(project_name))
                    .cloned(),
            )
            .await?
            {
                outcomes.push(outcome);
            }
        }
    }
    Ok(outcomes)
}

pub async fn schedule_trigger_evaluation(
    store: &Store,
    project_name: &str,
    trigger_id: i64,
) -> Result<AutomationTriggerView> {
    let project_id = projects::project_id(store, project_name).await?;
    let trigger = AutomationTrigger::find_by_id(trigger_id)
        .filter(automation_trigger::Column::ProjectId.eq(project_id))
        .one(store.db().as_ref())
        .await
        .context("failed to load automation trigger")?
        .ok_or_else(|| report!("trigger {trigger_id} does not exist in this project"))?;
    let now = utc_now();
    let pending_evaluation_count = trigger.pending_evaluation_count.saturating_add(1);
    let mut active: AutomationTriggerActiveModel = trigger.into();
    active.pending_evaluation_count = Set(pending_evaluation_count);
    active.last_evaluation_queued_at = Set(Some(now.clone()));
    active.updated_at = Set(now);
    let updated = active
        .update(store.db().as_ref())
        .await
        .context("failed to queue automation evaluation")?;
    events::publish_automation_changed(project_name);
    model_to_view(updated)
}

async fn run_queued_evaluations(
    store: &Store,
    sessions: Option<ProcessSessionRegistry>,
    project_cancellations: Option<&HashMap<String, watch::Receiver<bool>>>,
) -> Result<Vec<TriggerRunOutcome>> {
    let triggers = AutomationTrigger::find()
        .filter(automation_trigger::Column::PendingEvaluationCount.gt(0))
        .order_by_desc(automation_trigger::Column::Priority)
        .order_by_asc(automation_trigger::Column::LastEvaluationQueuedAt)
        .order_by_asc(automation_trigger::Column::Id)
        .all(store.db().as_ref())
        .await
        .context("failed to load queued automation evaluations")?;

    let mut outcomes = Vec::new();
    for trigger in triggers {
        let project_name = project_name_for_id(store, trigger.project_id).await?;
        let view = model_to_view(trigger.clone())?;
        if view.effect == AutomationEffect::ConsumeWork
            && !automation::can_start_automation_run(store, &project_name).await?
        {
            continue;
        }
        let trigger = consume_queued_evaluation(store, trigger).await?;
        if let Some(outcome) = evaluate_trigger_once(
            store,
            &project_name,
            trigger,
            None,
            sessions.clone(),
            project_cancellations
                .and_then(|cancellations| cancellations.get(&project_name))
                .cloned(),
        )
        .await
        {
            outcomes.push(outcome);
        }
    }
    Ok(outcomes)
}

async fn consume_queued_evaluation(
    store: &Store,
    trigger: AutomationTriggerModel,
) -> Result<AutomationTriggerModel> {
    let pending_evaluation_count = trigger.pending_evaluation_count.saturating_sub(1);
    let mut active: AutomationTriggerActiveModel = trigger.into();
    active.pending_evaluation_count = Set(pending_evaluation_count);
    active.updated_at = Set(utc_now());
    Ok(active
        .update(store.db().as_ref())
        .await
        .context("failed to consume queued automation evaluation")?)
}

async fn run_next_work_item_automation_for_project(
    store: &Store,
    project_name: &str,
    sessions: Option<ProcessSessionRegistry>,
    cancellation: Option<watch::Receiver<bool>>,
) -> Result<Option<TriggerRunOutcome>> {
    if !automation::can_start_automation_run(store, project_name).await? {
        return Ok(None);
    }
    let project_id = projects::project_id(store, project_name).await?;
    let triggers = AutomationTrigger::find()
        .filter(automation_trigger::Column::ProjectId.eq(project_id))
        .filter(automation_trigger::Column::Enabled.eq(true))
        .filter(
            automation_trigger::Column::Activation.eq(AutomationActivation::WorkItem.as_storage()),
        )
        .filter(automation_trigger::Column::Effect.eq(AutomationEffect::ConsumeWork.as_storage()))
        .order_by_asc(automation_trigger::Column::Id)
        .all(store.db().as_ref())
        .await
        .context("failed to load work-item automation entries")?;

    let mut candidates = Vec::new();
    let mut checked_without_match = Vec::new();
    for trigger in triggers {
        let view = model_to_view(trigger.clone())?;
        if !trigger_is_due(view.next_evaluation_at.as_deref()) {
            continue;
        }
        let Some(selector) = view.work_item_selector.as_ref() else {
            checked_without_match.push(trigger);
            continue;
        };
        if items::has_unclaimed_item_matching_condition(store, project_name, selector).await? {
            candidates.push(WorkItemAutomationCandidate { trigger, view });
        } else {
            checked_without_match.push(trigger);
        }
    }

    let Some(max_evaluation_count) = candidates
        .iter()
        .map(|candidate| candidate.view.evaluation_count)
        .max()
    else {
        for trigger in checked_without_match {
            update_trigger_after_check(store, trigger).await?;
        }
        return Ok(None);
    };
    let now = OffsetDateTime::now_utc();
    candidates.sort_by(|left, right| {
        work_item_automation_score(&right.view, max_evaluation_count, now)
            .cmp(&work_item_automation_score(
                &left.view,
                max_evaluation_count,
                now,
            ))
            .then_with(|| left.view.evaluation_count.cmp(&right.view.evaluation_count))
            .then_with(|| left.view.id.cmp(&right.view.id))
    });

    let candidate = candidates.remove(0);
    Ok(Some(
        evaluate_trigger_once(
            store,
            project_name,
            candidate.trigger,
            None,
            sessions,
            cancellation,
        )
        .await
        .expect("work-item automation candidate should produce an outcome"),
    ))
}

struct WorkItemAutomationCandidate {
    trigger: AutomationTriggerModel,
    view: AutomationTriggerView,
}

fn work_item_automation_score(
    automation: &AutomationTriggerView,
    max_evaluation_count: i64,
    now: OffsetDateTime,
) -> i64 {
    let age_seconds = automation
        .last_evaluated_at
        .as_deref()
        .and_then(|last_evaluated_at| OffsetDateTime::parse(last_evaluated_at, &Rfc3339).ok())
        .map(|last_evaluated_at| (now - last_evaluated_at).whole_seconds().max(0))
        .unwrap_or(NEVER_RUN_SCORE_SECONDS);
    let evaluation_count_gap = max_evaluation_count.saturating_sub(automation.evaluation_count);
    age_seconds
        .saturating_add(evaluation_count_gap.saturating_mul(EVALUATION_COUNT_SCORE_SECONDS))
        .saturating_add(automation.priority.saturating_mul(PRIORITY_SCORE_SECONDS))
}

pub fn spawn_scheduler_until(
    store: Store,
    sessions: Option<ProcessSessionRegistry>,
    controller: AutomationController,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    tokio::spawn(async move {
        let mut automation_interval =
            tokio::time::interval(StdDuration::from_secs(SCHEDULER_TICK_SECONDS));
        let mut maintenance_interval =
            tokio::time::interval(StdDuration::from_secs(MAINTENANCE_TICK_SECONDS));
        loop {
            tokio::select! {
                _ = automation_interval.tick() => {
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
                }
                _ = maintenance_interval.tick() => {
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

async fn evaluate_trigger_once(
    store: &Store,
    project_name: &str,
    trigger: AutomationTriggerModel,
    work_item_id: Option<i64>,
    sessions: Option<ProcessSessionRegistry>,
    cancellation: Option<watch::Receiver<bool>>,
) -> Option<TriggerRunOutcome> {
    let view = match model_to_view(trigger.clone()) {
        Ok(view) => view,
        Err(err) => {
            return Some(TriggerRunOutcome {
                trigger_id: trigger.id,
                trigger_name: trigger.name,
                work_item_id,
                work_item: None,
                run: None,
                error: Some(err.to_string()),
            });
        }
    };

    match view.effect {
        AutomationEffect::ProduceWork => {
            let result = create_work_item_from_trigger(store, project_name, &view).await;
            let _ = update_trigger_after_evaluation(store, trigger).await;
            let (work_item_id, work_item, error) = match result {
                Ok(work_item) => (Some(work_item.id), Some(work_item), None),
                Err(err) => (None, None, Some(err.to_string())),
            };
            Some(TriggerRunOutcome {
                trigger_id: view.id,
                trigger_name: view.name,
                work_item_id,
                work_item,
                run: None,
                error,
            })
        }
        AutomationEffect::ConsumeWork => {
            match trigger_has_consumable_work(store, project_name, &view, work_item_id).await {
                Ok(true) => Some(
                    run_trigger_once(
                        store,
                        project_name,
                        trigger,
                        work_item_id,
                        sessions,
                        cancellation,
                    )
                    .await,
                ),
                Ok(false) => {
                    let _ = update_trigger_after_check(store, trigger).await;
                    None
                }
                Err(err) => {
                    let _ = update_trigger_after_check(store, trigger).await;
                    Some(TriggerRunOutcome {
                        trigger_id: view.id,
                        trigger_name: view.name,
                        work_item_id,
                        work_item: None,
                        run: None,
                        error: Some(err.to_string()),
                    })
                }
            }
        }
    }
}

async fn create_work_item_from_trigger(
    store: &Store,
    project_name: &str,
    automation: &AutomationTriggerView,
) -> Result<crate::shared::view_models::WorkItemView> {
    items::create_item(
        store,
        project_name,
        CreateWorkItem {
            title: automation.name.clone(),
            description: automation.prompt.clone(),
            state: crate::shared::view_models::DEFAULT_STATE_LABEL.to_owned(),
            agent_model_override: None,
            agent_reasoning_effort_override: None,
        },
    )
    .await
}

async fn trigger_has_consumable_work(
    store: &Store,
    project_name: &str,
    automation: &AutomationTriggerView,
    work_item_id: Option<i64>,
) -> Result<bool> {
    if !automation.mode.claims_work() {
        return Ok(false);
    }
    if let Some(work_item_id) = work_item_id {
        let item = items::get_item(store, project_name, work_item_id).await?;
        return Ok(item.claimed_by.is_none() && item.finished_at.is_none());
    }
    let Some(selector) = automation.work_item_selector.as_ref() else {
        return Ok(false);
    };
    items::has_unclaimed_item_matching_condition(store, project_name, selector).await
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
                work_item: None,
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
            work_item_selector: view.work_item_selector.clone(),
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
    let outcome_work_item_id = run
        .as_ref()
        .and_then(|run| run.work_item_id)
        .or(work_item_id);

    TriggerRunOutcome {
        trigger_id: view.id,
        trigger_name: view.name,
        work_item_id: outcome_work_item_id,
        work_item: None,
        run,
        error,
    }
}

async fn update_trigger_after_evaluation(
    store: &Store,
    trigger: AutomationTriggerModel,
) -> Result<AutomationTriggerModel> {
    let view = model_to_view(trigger.clone())?;
    let now = utc_now();
    let next = match view.activation {
        AutomationActivation::WorkItem | AutomationActivation::Cron => {
            Some(next_evaluation_at(&view.schedule)?)
        }
        AutomationActivation::Manual => view.next_evaluation_at,
        AutomationActivation::WorkItemCreated => view.next_evaluation_at,
    };
    let evaluation_count = trigger.evaluation_count.saturating_add(1);
    let mut active: AutomationTriggerActiveModel = trigger.into();
    active.last_evaluated_at = Set(Some(now.clone()));
    active.next_evaluation_at = Set(next);
    active.evaluation_count = Set(evaluation_count);
    active.updated_at = Set(now);
    let updated = active
        .update(store.db().as_ref())
        .await
        .context("failed to update automation trigger after evaluation")?;
    publish_project_id_event(store, updated.project_id).await;
    Ok(updated)
}

async fn update_trigger_after_run(
    store: &Store,
    trigger: AutomationTriggerModel,
) -> Result<AutomationTriggerModel> {
    update_trigger_after_evaluation(store, trigger).await
}

async fn update_trigger_after_check(
    store: &Store,
    trigger: AutomationTriggerModel,
) -> Result<AutomationTriggerModel> {
    let view = model_to_view(trigger.clone())?;
    let mut active: AutomationTriggerActiveModel = trigger.into();
    let next = match view.activation {
        AutomationActivation::WorkItem | AutomationActivation::Cron => {
            Some(next_evaluation_at(&view.schedule)?)
        }
        AutomationActivation::Manual | AutomationActivation::WorkItemCreated => {
            view.next_evaluation_at
        }
    };
    active.next_evaluation_at = Set(next);
    active.updated_at = Set(utc_now());
    let updated = active
        .update(store.db().as_ref())
        .await
        .context("failed to update automation trigger after check")?;
    publish_project_id_event(store, updated.project_id).await;
    Ok(updated)
}

async fn update_trigger_event_cursor(
    store: &Store,
    trigger: AutomationTriggerModel,
    last_event_id: Option<i64>,
) -> Result<AutomationTriggerModel> {
    let mut active: AutomationTriggerActiveModel = trigger.into();
    active.last_event_id = Set(last_event_id);
    active.updated_at = Set(utc_now());
    let updated = active
        .update(store.db().as_ref())
        .await
        .context("failed to update automation trigger event cursor")?;
    publish_project_id_event(store, updated.project_id).await;
    Ok(updated)
}

async fn publish_project_id_event(store: &Store, project_id: i64) {
    match projects::project_name_by_id(store, project_id).await {
        Ok(project_name) => events::publish_automation_changed(&project_name),
        Err(err) => {
            eprintln!("failed to resolve project for automation trigger UI event: {err:#}");
        }
    }
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
    Ok(query
        .all(store.db().as_ref())
        .await
        .context("failed to load item-created events")?)
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
        .ok_or_else(|| report!("project {project_id} does not exist"))?;
    Ok(project.name)
}

fn normalize_schedule(schedule: String) -> Result<String> {
    let schedule = schedule.trim();
    if schedule.is_empty() {
        bail!("automation schedule is required");
    }
    parse_schedule(schedule)?;
    Ok(schedule.to_owned())
}

pub(crate) fn validate_trigger_fields(
    name: &str,
    _activation: AutomationActivation,
    schedule: &str,
) -> Result<()> {
    if name.trim().is_empty() {
        bail!("automation trigger name cannot be empty");
    }
    parse_schedule(schedule)?;
    Ok(())
}

pub(crate) fn validate_trigger_configuration(
    name: &str,
    activation: AutomationActivation,
    effect: AutomationEffect,
    schedule: &str,
    mode: AutomationMode,
    work_item_selector: Option<&Condition>,
    prompt: &str,
) -> Result<()> {
    validate_trigger_fields(name, activation, schedule)?;
    if let Some(condition) = work_item_selector {
        items::validate_label_condition(condition)?;
    }
    if effect == AutomationEffect::ProduceWork {
        if matches!(
            activation,
            AutomationActivation::WorkItem | AutomationActivation::WorkItemCreated
        ) {
            bail!("work-producing automation must use manual or cron activation");
        }
        if prompt.trim().is_empty() {
            bail!("work-producing automation requires prompt text for the created item");
        }
        return Ok(());
    }
    if !mode.claims_work() {
        bail!("work-consuming automation must use execute or refine mode");
    }
    if activation != AutomationActivation::WorkItemCreated && work_item_selector.is_none() {
        bail!("work-consuming automation requires a work item selector");
    }
    Ok(())
}

pub(crate) fn default_mode_for_activation(activation: AutomationActivation) -> AutomationMode {
    match activation {
        AutomationActivation::Manual => AutomationMode::Execute,
        AutomationActivation::WorkItem => AutomationMode::Execute,
        AutomationActivation::Cron => AutomationMode::Execute,
        AutomationActivation::WorkItemCreated => AutomationMode::Refine,
    }
}

pub(crate) fn default_work_item_selector() -> Condition {
    default_automation_work_item_selector()
}

pub(crate) fn default_work_item_selector_storage() -> Result<String> {
    selector_to_storage(Some(&default_work_item_selector()))?
        .ok_or_else(|| report!("default work-item automation selector cannot be empty"))
}

pub(crate) async fn ensure_default_project_automations_in_conn<C>(
    conn: &C,
    project_id: i64,
    default_tool: &str,
) -> Result<()>
where
    C: ConnectionTrait,
{
    let existing = AutomationTrigger::find()
        .filter(automation_trigger::Column::ProjectId.eq(project_id))
        .filter(
            automation_trigger::Column::Activation.eq(AutomationActivation::WorkItem.as_storage()),
        )
        .limit(1)
        .one(conn)
        .await
        .context("failed to check project automation defaults")?;
    if existing.is_some() {
        return Ok(());
    }

    let now = utc_now();
    AutomationTriggerActiveModel {
        project_id: Set(project_id),
        name: Set(DEFAULT_WORK_ITEM_AUTOMATION_NAME.to_owned()),
        enabled: Set(true),
        activation: Set(AutomationActivation::WorkItem.as_storage().to_owned()),
        effect: Set(AutomationEffect::ConsumeWork.as_storage().to_owned()),
        schedule: Set(DEFAULT_WORK_ITEM_AUTOMATION_SCHEDULE.to_owned()),
        mode: Set(AutomationMode::Execute.as_storage().to_owned()),
        tool_name: Set(default_tool.to_owned()),
        prompt: Set(String::new()),
        work_item_selector: Set(Some(default_work_item_selector_storage()?)),
        priority: Set(0),
        evaluation_count: Set(0),
        pending_evaluation_count: Set(0),
        last_evaluation_queued_at: Set(None),
        last_evaluated_at: Set(None),
        next_evaluation_at: Set(None),
        last_event_id: Set(None),
        created_at: Set(now.clone()),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(conn)
    .await
    .context("failed to create default project automation")?;

    Ok(())
}

pub(crate) async fn ensure_default_project_automations(
    store: &Store,
    project_id: i64,
    default_tool: &str,
) -> Result<()> {
    ensure_default_project_automations_in_conn(store.db().as_ref(), project_id, default_tool).await
}

fn selector_for_activation(
    activation: AutomationActivation,
    selector: Option<Condition>,
) -> Result<Option<Condition>> {
    match (activation, selector) {
        (AutomationActivation::WorkItem, None) => Ok(Some(default_work_item_selector())),
        (_, selector) => Ok(selector),
    }
}

pub(crate) fn selector_to_storage(selector: Option<&Condition>) -> Result<Option<String>> {
    selector
        .map(|selector| -> Result<String> {
            items::validate_label_condition(selector)?;
            Ok(serde_json::to_string(selector).context("failed to encode work item selector")?)
        })
        .transpose()
}

pub(crate) fn selector_from_storage(selector: Option<&str>) -> Result<Option<Condition>> {
    selector
        .and_then(|selector| {
            let selector = selector.trim();
            (!selector.is_empty()).then_some(selector)
        })
        .map(|selector| {
            let condition = serde_json::from_str::<Condition>(selector)
                .context_with(|| format!("invalid work item selector JSON: {selector}"))?;
            items::validate_label_condition(&condition)?;
            Ok(condition)
        })
        .transpose()
}

fn trigger_is_due(next_evaluation_at: Option<&str>) -> bool {
    let Some(next_evaluation_at) = next_evaluation_at else {
        return true;
    };
    let Ok(next) = OffsetDateTime::parse(next_evaluation_at, &Rfc3339) else {
        return true;
    };
    next <= OffsetDateTime::now_utc()
}

pub(crate) fn next_evaluation_at(schedule: &str) -> Result<String> {
    let interval = parse_schedule(schedule)?;
    Ok((OffsetDateTime::now_utc() + interval)
        .format(&Rfc3339)
        .context("failed to format next trigger run time")?)
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
        .context_with(|| format!("invalid schedule amount '{number}'"))?;
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
        activation: AutomationActivation::from_str(&trigger.activation)?,
        effect: AutomationEffect::from_str(&trigger.effect)?,
        schedule: trigger.schedule,
        mode: AutomationMode::from_str(&trigger.mode)?,
        tool_name: AgentToolName::from_str(&trigger.tool_name)?,
        prompt: trigger.prompt,
        work_item_selector: selector_from_storage(trigger.work_item_selector.as_deref())?,
        priority: trigger.priority,
        evaluation_count: trigger.evaluation_count,
        pending_evaluation_count: trigger.pending_evaluation_count,
        last_evaluation_queued_at: trigger.last_evaluation_queued_at,
        last_evaluated_at: trigger.last_evaluated_at,
        next_evaluation_at: trigger.next_evaluation_at,
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
                default_agent_reasoning_effort: None,
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
    async fn new_project_gets_default_work_item_automation() {
        let (_temp, store) = test_store().await;
        let automations = list_triggers(&store, "demo").await.unwrap();
        let automation = automations
            .iter()
            .find(|automation| automation.name == DEFAULT_WORK_ITEM_AUTOMATION_NAME)
            .unwrap();

        assert_eq!(automation.activation, AutomationActivation::WorkItem);
        assert_eq!(automation.effect, AutomationEffect::ConsumeWork);
        assert_eq!(automation.schedule, DEFAULT_WORK_ITEM_AUTOMATION_SCHEDULE);
        assert_eq!(automation.mode, AutomationMode::Execute);
        assert_eq!(
            automation.work_item_selector,
            Some(default_work_item_selector())
        );
        assert_eq!(automation.priority, 0);
        assert_eq!(automation.evaluation_count, 0);
        assert_eq!(automation.pending_evaluation_count, 0);
    }

    #[test]
    fn work_item_automation_score_combines_age_evaluation_count_and_priority() {
        let now = OffsetDateTime::now_utc();
        let stale_low_priority = automation_view_for_score(
            1,
            Some((now - Duration::minutes(30)).format(&Rfc3339).unwrap()),
            10,
            0,
        );
        let recent_lower_evaluation_count = automation_view_for_score(
            2,
            Some((now - Duration::minutes(1)).format(&Rfc3339).unwrap()),
            8,
            0,
        );
        let recent_high_priority = automation_view_for_score(
            3,
            Some((now - Duration::minutes(1)).format(&Rfc3339).unwrap()),
            10,
            10,
        );

        assert!(
            work_item_automation_score(&stale_low_priority, 10, now)
                > work_item_automation_score(&recent_lower_evaluation_count, 10, now)
        );
        assert!(
            work_item_automation_score(&recent_lower_evaluation_count, 10, now)
                > work_item_automation_score(&recent_high_priority, 10, now)
                    - (10 * PRIORITY_SCORE_SECONDS)
        );
        assert!(
            work_item_automation_score(&recent_high_priority, 10, now)
                > work_item_automation_score(&recent_lower_evaluation_count, 10, now)
        );
    }

    fn automation_view_for_score(
        id: i64,
        last_evaluated_at: Option<String>,
        evaluation_count: i64,
        priority: i64,
    ) -> AutomationTriggerView {
        AutomationTriggerView {
            id,
            project_id: 1,
            name: format!("automation-{id}"),
            enabled: true,
            activation: AutomationActivation::WorkItem,
            effect: AutomationEffect::ConsumeWork,
            schedule: DEFAULT_WORK_ITEM_AUTOMATION_SCHEDULE.to_owned(),
            mode: AutomationMode::Execute,
            tool_name: AgentToolName::Codex,
            prompt: String::new(),
            work_item_selector: Some(default_work_item_selector()),
            priority,
            evaluation_count,
            pending_evaluation_count: 0,
            last_evaluation_queued_at: None,
            last_evaluated_at,
            next_evaluation_at: None,
            last_event_id: None,
            created_at: "2026-06-15T00:00:00Z".to_owned(),
            updated_at: "2026-06-15T00:00:00Z".to_owned(),
        }
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
                activation: AutomationActivation::WorkItemCreated,
                effect: AutomationEffect::ConsumeWork,
                schedule: "@every 15s".to_owned(),
                mode: None,
                tool_name: None,
                prompt: "Refine this new work item.".to_owned(),
                work_item_selector: None,
                priority: 0,
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
                state: "open".to_owned(),
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
        assert_eq!(item.state.as_deref(), Some("open"));
    }

    #[tokio::test]
    async fn queued_work_producing_trigger_creates_item_without_agent_run() {
        let (_temp, store) = test_store().await;
        let trigger = create_trigger(
            &store,
            "demo",
            CreateAutomationTrigger {
                name: "deep-review".to_owned(),
                enabled: true,
                activation: AutomationActivation::Manual,
                effect: AutomationEffect::ProduceWork,
                schedule: "@every 15s".to_owned(),
                mode: None,
                tool_name: None,
                prompt: "Perform an expensive deep review.".to_owned(),
                work_item_selector: None,
                priority: 100,
            },
        )
        .await
        .unwrap();
        let trigger_id = trigger.id;

        let queued = schedule_trigger_evaluation(&store, "demo", trigger_id)
            .await
            .unwrap();
        assert_eq!(queued.pending_evaluation_count, 1);

        let outcomes = run_due_triggers(&store).await.unwrap();
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].trigger_id, trigger_id);
        assert!(outcomes[0].run.is_none());

        let work_item = outcomes[0].work_item.as_ref().unwrap();
        assert_eq!(outcomes[0].work_item_id, Some(work_item.id));
        assert_eq!(work_item.title, "deep-review");
        assert_eq!(work_item.description, "Perform an expensive deep review.");

        let trigger = list_triggers(&store, "demo")
            .await
            .unwrap()
            .into_iter()
            .find(|trigger| trigger.id == trigger_id)
            .unwrap();
        assert_eq!(trigger.pending_evaluation_count, 0);
        assert_eq!(trigger.evaluation_count, 1);
    }
}
