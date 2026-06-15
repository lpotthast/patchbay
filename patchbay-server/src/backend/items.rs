use std::str::FromStr;

use anyhow::{Context, Result, bail};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, EntityTrait, PaginatorTrait,
    QueryFilter, QueryOrder, Statement, TransactionTrait,
};
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
    backend::{
        entities::{
            comment::{self, CommentActiveModel, CommentModel},
            work_item::{self, WorkItem, WorkItemActiveModel, WorkItemModel},
            work_item_event::{self, WorkItemEventActiveModel},
        },
        projects,
        storage::{Store, utc_now},
    },
    shared::view_models::{
        AgentReasoningEffort, AuthorType, CommentView, RecoveredClaimView, WorkItemEventView,
        WorkItemView, WorkState,
    },
};

#[derive(Clone, Debug)]
pub struct CreateWorkItem {
    pub title: String,
    pub description: String,
    pub automation_claimable: bool,
    pub agent_model_override: Option<String>,
    pub agent_reasoning_effort_override: Option<AgentReasoningEffort>,
}

#[derive(Clone, Debug, Default)]
pub struct UpdateWorkItem {
    pub title: Option<String>,
    pub description: Option<String>,
    pub automation_claimable: Option<bool>,
    pub agent_model_override: Option<Option<String>>,
    pub agent_reasoning_effort_override: Option<Option<AgentReasoningEffort>>,
    pub expect_version: Option<i64>,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct EventAttribution<'a> {
    pub actor_type: Option<&'a str>,
    pub actor_id: Option<&'a str>,
    pub agent_run_id: Option<i64>,
}

pub async fn list_items(
    store: &Store,
    project_name: &str,
    state: Option<WorkState>,
) -> Result<Vec<WorkItemView>> {
    let project_id = projects::project_id(store, project_name).await?;
    let mut query = WorkItem::find()
        .filter(work_item::Column::ProjectId.eq(project_id))
        .order_by_asc(work_item::Column::State)
        .order_by_desc(work_item::Column::UpdatedAt)
        .order_by_desc(work_item::Column::Id);

    if let Some(state) = state {
        query = query.filter(work_item::Column::State.eq(state.as_storage()));
    }

    let items = query
        .all(store.db().as_ref())
        .await
        .context("failed to list work items")?;
    models_to_views(store, items).await
}

pub async fn has_claimable_item(
    store: &Store,
    project_name: &str,
    state: WorkState,
) -> Result<bool> {
    let project_id = projects::project_id(store, project_name).await?;
    let count = WorkItem::find()
        .filter(work_item::Column::ProjectId.eq(project_id))
        .filter(work_item::Column::State.eq(state.as_storage()))
        .filter(work_item::Column::ClaimedBy.is_null())
        .filter(work_item::Column::AutomationClaimable.eq(true))
        .count(store.db().as_ref())
        .await
        .context("failed to count claimable work items")?;
    Ok(count > 0)
}

pub async fn get_item(store: &Store, project_name: &str, item_id: i64) -> Result<WorkItemView> {
    let project_id = projects::project_id(store, project_name).await?;
    let item = get_item_model(store, project_id, item_id).await?;
    model_to_view(store, item).await
}

pub async fn create_item(
    store: &Store,
    project_name: &str,
    create: CreateWorkItem,
) -> Result<WorkItemView> {
    validate_item_text(&create.title, &create.description)?;
    let agent_model_override = projects::normalize_optional(create.agent_model_override);

    let project_id = projects::project_id(store, project_name).await?;
    let now = utc_now();
    let txn = store
        .db()
        .begin()
        .await
        .context("failed to start item create")?;

    let active = WorkItemActiveModel {
        project_id: Set(project_id),
        title: Set(create.title),
        description: Set(create.description),
        state: Set(WorkState::Open.as_storage().to_owned()),
        automation_claimable: Set(create.automation_claimable),
        agent_model_override: Set(agent_model_override),
        agent_reasoning_effort_override: Set(create
            .agent_reasoning_effort_override
            .map(|effort| effort.as_storage().to_owned())),
        version: Set(1),
        created_at: Set(now.clone()),
        updated_at: Set(now),
        ..Default::default()
    };
    let item = active
        .insert(&txn)
        .await
        .context("failed to create work item")?;
    record_event_in_tx(
        &txn,
        project_id,
        Some(item.id),
        "item_created",
        "Created item",
    )
    .await?;
    txn.commit().await.context("failed to commit item create")?;

    model_to_view(store, item).await
}

pub async fn update_item(
    store: &Store,
    project_name: &str,
    item_id: i64,
    update: UpdateWorkItem,
) -> Result<WorkItemView> {
    if update.title.is_none()
        && update.description.is_none()
        && update.automation_claimable.is_none()
        && update.agent_model_override.is_none()
        && update.agent_reasoning_effort_override.is_none()
    {
        bail!("item update requires at least one field");
    }

    let project_id = projects::project_id(store, project_name).await?;
    let txn = store
        .db()
        .begin()
        .await
        .context("failed to start item update")?;
    let existing = get_item_model_in_tx(&txn, project_id, item_id).await?;
    check_expected_version(update.expect_version, existing.version)?;

    let title = update.title.unwrap_or_else(|| existing.title.clone());
    let description = update
        .description
        .unwrap_or_else(|| existing.description.clone());
    validate_item_text(&title, &description)?;

    let version = existing.version;
    let mut active: WorkItemActiveModel = existing.into();
    active.title = Set(title);
    active.description = Set(description);
    if let Some(automation_claimable) = update.automation_claimable {
        active.automation_claimable = Set(automation_claimable);
    }
    if let Some(agent_model_override) = update.agent_model_override {
        active.agent_model_override = Set(projects::normalize_optional(agent_model_override));
    }
    if let Some(agent_reasoning_effort_override) = update.agent_reasoning_effort_override {
        active.agent_reasoning_effort_override =
            Set(agent_reasoning_effort_override.map(|effort| effort.as_storage().to_owned()));
    }
    active.version = Set(version + 1);
    active.updated_at = Set(utc_now());

    let updated = active
        .update(&txn)
        .await
        .context("failed to update work item")?;
    record_event_in_tx(
        &txn,
        project_id,
        Some(item_id),
        "item_updated",
        "Updated item",
    )
    .await?;
    txn.commit().await.context("failed to commit item update")?;

    model_to_view(store, updated).await
}

pub async fn move_item(
    store: &Store,
    project_name: &str,
    item_id: i64,
    state: WorkState,
    expect_version: Option<i64>,
) -> Result<WorkItemView> {
    let project_id = projects::project_id(store, project_name).await?;
    let txn = store
        .db()
        .begin()
        .await
        .context("failed to start item move")?;
    let existing = get_item_model_in_tx(&txn, project_id, item_id).await?;
    check_expected_version(expect_version, existing.version)?;

    let version = existing.version;
    let mut active: WorkItemActiveModel = existing.into();
    active.state = Set(state.as_storage().to_owned());
    active.version = Set(version + 1);
    active.updated_at = Set(utc_now());

    let updated = active
        .update(&txn)
        .await
        .context("failed to move work item")?;
    let event_body = format!("Moved item to {}", state.label());
    record_event_in_tx(&txn, project_id, Some(item_id), "item_moved", &event_body).await?;
    txn.commit().await.context("failed to commit item move")?;

    model_to_view(store, updated).await
}

pub async fn claim_item(
    store: &Store,
    project_name: &str,
    agent_id: &str,
    state_filter: WorkState,
) -> Result<Option<WorkItemView>> {
    validate_agent_id(agent_id)?;

    let project_id = projects::project_id(store, project_name).await?;
    let txn = store
        .db()
        .begin()
        .await
        .context("failed to start item claim")?;
    let now = utc_now();

    let claimed_id = txn
        .query_one(Statement::from_sql_and_values(
            sea_orm::DbBackend::Sqlite,
            r#"
            UPDATE work_items
            SET state = 'in_progress',
                claimed_by = ?2,
                claimed_at = ?3,
                claim_expires_at = NULL,
                finished_at = NULL,
                version = version + 1,
                updated_at = ?3
            WHERE id = (
                SELECT id
                FROM work_items
                WHERE project_id = ?1
                  AND state = ?4
                  AND claimed_by IS NULL
                  AND automation_claimable = 1
                ORDER BY updated_at ASC, id ASC
                LIMIT 1
            )
            RETURNING id
            "#,
            vec![
                project_id.into(),
                agent_id.to_owned().into(),
                now.clone().into(),
                state_filter.as_storage().to_owned().into(),
            ],
        ))
        .await
        .context("failed to claim work item")?
        .map(|row| row.try_get::<i64>("", "id"))
        .transpose()
        .context("failed to read claimed item id")?;

    let Some(item_id) = claimed_id else {
        txn.commit().await.context("failed to commit empty claim")?;
        return Ok(None);
    };

    let comment_body = format!("Claimed by {agent_id}");
    insert_comment_in_tx(
        &txn,
        item_id,
        AuthorType::System,
        None,
        comment_body.as_str(),
    )
    .await?;
    record_event_in_tx(
        &txn,
        project_id,
        Some(item_id),
        "item_claimed",
        comment_body.as_str(),
    )
    .await?;
    let item = get_item_model_in_tx(&txn, project_id, item_id).await?;
    txn.commit().await.context("failed to commit item claim")?;

    Ok(Some(model_to_view(store, item).await?))
}

pub async fn claim_specific_item(
    store: &Store,
    project_name: &str,
    item_id: i64,
    agent_id: &str,
) -> Result<Option<WorkItemView>> {
    validate_agent_id(agent_id)?;

    let project_id = projects::project_id(store, project_name).await?;
    let txn = store
        .db()
        .begin()
        .await
        .context("failed to start specific item claim")?;
    let now = utc_now();

    let claimed_id = txn
        .query_one(Statement::from_sql_and_values(
            sea_orm::DbBackend::Sqlite,
            r#"
            UPDATE work_items
            SET state = 'in_progress',
                claimed_by = ?3,
                claimed_at = ?4,
                claim_expires_at = NULL,
                finished_at = NULL,
                version = version + 1,
                updated_at = ?4
            WHERE id = ?2
              AND project_id = ?1
              AND state = 'open'
              AND claimed_by IS NULL
              AND automation_claimable = 1
            RETURNING id
            "#,
            vec![
                project_id.into(),
                item_id.into(),
                agent_id.to_owned().into(),
                now.clone().into(),
            ],
        ))
        .await
        .context("failed to claim specific work item")?
        .map(|row| row.try_get::<i64>("", "id"))
        .transpose()
        .context("failed to read claimed item id")?;

    let Some(item_id) = claimed_id else {
        txn.commit()
            .await
            .context("failed to commit empty specific claim")?;
        return Ok(None);
    };

    let comment_body = format!("Claimed by {agent_id}");
    insert_comment_in_tx(
        &txn,
        item_id,
        AuthorType::System,
        None,
        comment_body.as_str(),
    )
    .await?;
    record_event_in_tx(
        &txn,
        project_id,
        Some(item_id),
        "item_claimed",
        comment_body.as_str(),
    )
    .await?;
    let item = get_item_model_in_tx(&txn, project_id, item_id).await?;
    txn.commit()
        .await
        .context("failed to commit specific item claim")?;

    Ok(Some(model_to_view(store, item).await?))
}

pub async fn release_item(
    store: &Store,
    project_name: &str,
    item_id: i64,
    agent_id: &str,
    comment: Option<String>,
) -> Result<WorkItemView> {
    validate_agent_id(agent_id)?;

    let project_id = projects::project_id(store, project_name).await?;
    let txn = store
        .db()
        .begin()
        .await
        .context("failed to start item release")?;
    let existing = get_item_model_in_tx(&txn, project_id, item_id).await?;
    ensure_active_claim(&existing, agent_id)?;

    let now = utc_now();
    let version = existing.version;
    let mut active: WorkItemActiveModel = existing.into();
    active.state = Set(WorkState::Open.as_storage().to_owned());
    active.claimed_by = Set(None);
    active.claimed_at = Set(None);
    active.claim_expires_at = Set(None);
    active.version = Set(version + 1);
    active.updated_at = Set(now);
    let updated = active
        .update(&txn)
        .await
        .context("failed to release work item")?;

    if let Some(comment) = comment.filter(|body| !body.trim().is_empty()) {
        insert_comment_in_tx(
            &txn,
            item_id,
            AuthorType::Agent,
            Some(agent_id.to_owned()),
            comment.as_str(),
        )
        .await?;
    }

    let event_body = format!("Released by {agent_id}");
    record_event_in_tx(
        &txn,
        project_id,
        Some(item_id),
        "item_released",
        event_body.as_str(),
    )
    .await?;
    txn.commit()
        .await
        .context("failed to commit item release")?;

    model_to_view(store, updated).await
}

pub async fn progress_item(
    store: &Store,
    project_name: &str,
    item_id: i64,
    agent_id: &str,
    body: &str,
) -> Result<CommentView> {
    validate_agent_id(agent_id)?;
    if body.trim().is_empty() {
        bail!("progress body cannot be empty");
    }

    let project_id = projects::project_id(store, project_name).await?;
    let txn = store
        .db()
        .begin()
        .await
        .context("failed to start item progress")?;
    let item = get_item_model_in_tx(&txn, project_id, item_id).await?;
    ensure_active_claim(&item, agent_id)?;

    let comment = insert_comment_in_tx(
        &txn,
        item_id,
        AuthorType::Agent,
        Some(agent_id.to_owned()),
        body,
    )
    .await?;

    let mut item_active: WorkItemActiveModel = item.clone().into();
    item_active.version = Set(item.version + 1);
    item_active.updated_at = Set(utc_now());
    item_active
        .update(&txn)
        .await
        .context("failed to update item after progress")?;
    record_event_in_tx(&txn, project_id, Some(item_id), "progress_added", body).await?;
    txn.commit()
        .await
        .context("failed to commit item progress")?;

    comment_to_view(comment)
}

pub async fn finish_item(
    store: &Store,
    project_name: &str,
    item_id: i64,
    agent_id: &str,
    report: &str,
) -> Result<WorkItemView> {
    validate_agent_id(agent_id)?;
    if report.trim().is_empty() {
        bail!("finish report cannot be empty");
    }

    let project_id = projects::project_id(store, project_name).await?;
    let txn = store
        .db()
        .begin()
        .await
        .context("failed to start item finish")?;
    let existing = get_item_model_in_tx(&txn, project_id, item_id).await?;
    ensure_active_claim(&existing, agent_id)?;

    let now = utc_now();
    insert_comment_in_tx(
        &txn,
        item_id,
        AuthorType::Agent,
        Some(agent_id.to_owned()),
        report,
    )
    .await?;

    let version = existing.version;
    let mut active: WorkItemActiveModel = existing.into();
    active.state = Set(WorkState::Done.as_storage().to_owned());
    active.claimed_by = Set(None);
    active.claimed_at = Set(None);
    active.claim_expires_at = Set(None);
    active.finished_at = Set(Some(now.clone()));
    active.version = Set(version + 1);
    active.updated_at = Set(now);
    let updated = active
        .update(&txn)
        .await
        .context("failed to finish work item")?;
    record_event_in_tx(&txn, project_id, Some(item_id), "item_finished", report).await?;
    txn.commit().await.context("failed to commit item finish")?;

    model_to_view(store, updated).await
}

pub async fn delete_item(store: &Store, project_name: &str, item_id: i64) -> Result<()> {
    let project_id = projects::project_id(store, project_name).await?;
    let txn = store
        .db()
        .begin()
        .await
        .context("failed to start item delete")?;
    get_item_model_in_tx(&txn, project_id, item_id).await?;

    record_event_in_tx(
        &txn,
        project_id,
        Some(item_id),
        "item_deleted",
        "Deleted item",
    )
    .await?;
    WorkItem::delete_by_id(item_id)
        .exec(&txn)
        .await
        .context("failed to delete work item")?;
    txn.commit().await.context("failed to commit item delete")?;
    Ok(())
}

pub async fn list_events(
    store: &Store,
    project_name: &str,
    item_id: Option<i64>,
    since_id: Option<i64>,
) -> Result<Vec<WorkItemEventView>> {
    let project_id = projects::project_id(store, project_name).await?;
    if let Some(item_id) = item_id {
        get_item_model(store, project_id, item_id).await?;
    }

    let mut query = work_item_event::Entity::find()
        .filter(work_item_event::Column::ProjectId.eq(project_id))
        .order_by_asc(work_item_event::Column::Id);

    if let Some(item_id) = item_id {
        query = query.filter(work_item_event::Column::WorkItemId.eq(item_id));
    }

    if let Some(since_id) = since_id {
        query = query.filter(work_item_event::Column::Id.gt(since_id));
    }

    let events = query
        .all(store.db().as_ref())
        .await
        .context("failed to list work item events")?;

    Ok(events
        .into_iter()
        .map(|event| WorkItemEventView {
            id: event.id,
            project_id: event.project_id,
            work_item_id: event.work_item_id,
            event_type: event.event_type,
            body: event.body,
            actor_type: event.actor_type,
            actor_id: event.actor_id,
            agent_run_id: event.agent_run_id,
            created_at: event.created_at,
        })
        .collect())
}

pub async fn recover_stale_claims(
    store: &Store,
    project_name: &str,
    stale_after_minutes: i64,
) -> Result<Vec<RecoveredClaimView>> {
    if stale_after_minutes <= 0 {
        return Ok(Vec::new());
    }

    let project_id = projects::project_id(store, project_name).await?;
    let items = WorkItem::find()
        .filter(work_item::Column::ProjectId.eq(project_id))
        .filter(work_item::Column::State.eq(WorkState::InProgress.as_storage()))
        .filter(work_item::Column::ClaimedBy.is_not_null())
        .all(store.db().as_ref())
        .await
        .context("failed to list claimed work items")?;
    let cutoff = OffsetDateTime::now_utc() - Duration::minutes(stale_after_minutes);
    let mut recovered = Vec::new();

    for item in items {
        let Some(agent_id) = item.claimed_by.clone() else {
            continue;
        };
        let stale = match item.claim_expires_at.as_deref() {
            Some(expires_at) => timestamp_is_before_or_equal(expires_at, OffsetDateTime::now_utc()),
            None => item
                .claimed_at
                .as_deref()
                .map(|claimed_at| timestamp_is_before_or_equal(claimed_at, cutoff))
                .unwrap_or(false),
        };
        if !stale {
            continue;
        }

        let claim = RecoveredClaimView {
            item_id: item.id,
            agent_id: agent_id.clone(),
            claimed_at: item.claimed_at.clone(),
        };
        release_item(
            store,
            project_name,
            item.id,
            &agent_id,
            Some(format!(
                "Recovered stale claim after {stale_after_minutes} minute(s)."
            )),
        )
        .await?;
        recovered.push(claim);
    }

    Ok(recovered)
}

pub(crate) async fn get_item_model(
    store: &Store,
    project_id: i64,
    item_id: i64,
) -> Result<WorkItemModel> {
    WorkItem::find_by_id(item_id)
        .filter(work_item::Column::ProjectId.eq(project_id))
        .one(store.db().as_ref())
        .await
        .with_context(|| format!("failed to load item {item_id}"))?
        .ok_or_else(|| anyhow::anyhow!("item {item_id} does not exist in this project"))
}

pub(crate) async fn get_item_model_in_tx<C>(
    conn: &C,
    project_id: i64,
    item_id: i64,
) -> Result<WorkItemModel>
where
    C: sea_orm::ConnectionTrait,
{
    WorkItem::find_by_id(item_id)
        .filter(work_item::Column::ProjectId.eq(project_id))
        .one(conn)
        .await
        .with_context(|| format!("failed to load item {item_id}"))?
        .ok_or_else(|| anyhow::anyhow!("item {item_id} does not exist in this project"))
}

pub(crate) async fn record_event_in_tx<C>(
    conn: &C,
    project_id: i64,
    work_item_id: Option<i64>,
    event_type: &str,
    body: &str,
) -> Result<work_item_event::Model>
where
    C: sea_orm::ConnectionTrait,
{
    record_event_with_attribution_in_tx(
        conn,
        project_id,
        work_item_id,
        event_type,
        body,
        EventAttribution::default(),
    )
    .await
}

pub(crate) async fn record_event_with_attribution_in_tx<C>(
    conn: &C,
    project_id: i64,
    work_item_id: Option<i64>,
    event_type: &str,
    body: &str,
    attribution: EventAttribution<'_>,
) -> Result<work_item_event::Model>
where
    C: sea_orm::ConnectionTrait,
{
    let active = WorkItemEventActiveModel {
        project_id: Set(project_id),
        work_item_id: Set(work_item_id),
        event_type: Set(event_type.to_owned()),
        body: Set(body.to_owned()),
        actor_type: Set(attribution.actor_type.map(ToOwned::to_owned)),
        actor_id: Set(attribution.actor_id.map(ToOwned::to_owned)),
        agent_run_id: Set(attribution.agent_run_id),
        created_at: Set(utc_now()),
        ..Default::default()
    };
    let event = active
        .insert(conn)
        .await
        .with_context(|| format!("failed to record event {event_type}"))?;
    Ok(event)
}

async fn insert_comment_in_tx<C>(
    conn: &C,
    item_id: i64,
    author_type: AuthorType,
    author_name: Option<String>,
    body: &str,
) -> Result<CommentModel>
where
    C: sea_orm::ConnectionTrait,
{
    let active = CommentActiveModel {
        work_item_id: Set(item_id),
        author_type: Set(author_type.as_storage().to_owned()),
        author_name: Set(author_name),
        body: Set(body.to_owned()),
        created_at: Set(utc_now()),
        ..Default::default()
    };
    active
        .insert(conn)
        .await
        .context("failed to add item comment")
}

async fn models_to_views(store: &Store, items: Vec<WorkItemModel>) -> Result<Vec<WorkItemView>> {
    let mut views = Vec::with_capacity(items.len());
    for item in items {
        views.push(model_to_view(store, item).await?);
    }
    Ok(views)
}

async fn model_to_view(store: &Store, item: WorkItemModel) -> Result<WorkItemView> {
    let state = WorkState::from_str(&item.state)?;
    let comment_count = comment::Comment::find()
        .filter(comment::Column::WorkItemId.eq(item.id))
        .count(store.db().as_ref())
        .await
        .context("failed to count item comments")? as i64;

    Ok(WorkItemView {
        id: item.id,
        project_id: item.project_id,
        title: item.title,
        description: item.description,
        state,
        version: item.version,
        claimed_by: item.claimed_by,
        claimed_at: item.claimed_at,
        claim_expires_at: item.claim_expires_at,
        finished_at: item.finished_at,
        automation_claimable: item.automation_claimable,
        agent_model_override: projects::normalize_optional(item.agent_model_override),
        agent_reasoning_effort_override: item
            .agent_reasoning_effort_override
            .as_deref()
            .map(str::parse::<AgentReasoningEffort>)
            .transpose()?,
        created_at: item.created_at,
        updated_at: item.updated_at,
        comment_count,
    })
}

fn validate_item_text(title: &str, description: &str) -> Result<()> {
    if title.trim().is_empty() {
        bail!("item title cannot be empty");
    }
    if description.trim().is_empty() {
        bail!("item description cannot be empty");
    }
    Ok(())
}

fn validate_agent_id(agent_id: &str) -> Result<()> {
    if agent_id.trim().is_empty() {
        bail!("agent id cannot be empty");
    }
    Ok(())
}

fn ensure_active_claim(item: &WorkItemModel, agent_id: &str) -> Result<()> {
    match item.claimed_by.as_deref() {
        Some(claimed_by) if claimed_by == agent_id => Ok(()),
        Some(claimed_by) => bail!("item {} is claimed by {claimed_by}", item.id),
        None => bail!("item {} is not claimed", item.id),
    }
}

fn check_expected_version(expected: Option<i64>, actual: i64) -> Result<()> {
    if let Some(expected) = expected
        && expected != actual
    {
        bail!("version conflict: expected {expected}, found {actual}");
    }
    Ok(())
}

fn timestamp_is_before_or_equal(value: &str, cutoff: OffsetDateTime) -> bool {
    OffsetDateTime::parse(value, &Rfc3339)
        .map(|timestamp| timestamp <= cutoff)
        .unwrap_or(false)
}

fn comment_to_view(comment: CommentModel) -> Result<CommentView> {
    Ok(CommentView {
        id: comment.id,
        work_item_id: comment.work_item_id,
        author_type: AuthorType::from_str(&comment.author_type)?,
        author_name: comment.author_name,
        body: comment.body,
        created_at: comment.created_at,
    })
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::backend::{
        comments::list_comments,
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
        create_project(
            &store,
            CreateProject {
                name: "other".to_owned(),
                display_name: None,
                path: temp.path().to_path_buf(),
                default_agent_model: None,
                system_prompt: None,
                memory: None,
            },
        )
        .await
        .unwrap();
        (temp, store)
    }

    #[tokio::test]
    async fn work_items_are_scoped_to_project() {
        let (_temp, store) = test_store().await;
        let demo_item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Demo item".to_owned(),
                description: "Build the demo item".to_owned(),
                automation_claimable: true,
                agent_model_override: None,
                agent_reasoning_effort_override: None,
            },
        )
        .await
        .unwrap();
        create_item(
            &store,
            "other",
            CreateWorkItem {
                title: "Other item".to_owned(),
                description: "Build the other item".to_owned(),
                automation_claimable: true,
                agent_model_override: None,
                agent_reasoning_effort_override: None,
            },
        )
        .await
        .unwrap();

        let demo_items = list_items(&store, "demo", None).await.unwrap();
        let other_lookup = get_item(&store, "other", demo_item.id).await.unwrap_err();

        assert_eq!(demo_items.len(), 1);
        assert_eq!(demo_items[0].title, "Demo item");
        assert!(
            other_lookup
                .to_string()
                .contains("does not exist in this project")
        );
    }

    #[tokio::test]
    async fn moving_item_updates_state_and_version() {
        let (_temp, store) = test_store().await;
        let item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Move me".to_owned(),
                description: "Move through states".to_owned(),
                automation_claimable: true,
                agent_model_override: None,
                agent_reasoning_effort_override: None,
            },
        )
        .await
        .unwrap();

        let moved = move_item(
            &store,
            "demo",
            item.id,
            WorkState::InProgress,
            Some(item.version),
        )
        .await
        .unwrap();

        assert_eq!(moved.state, WorkState::InProgress);
        assert_eq!(moved.version, item.version + 1);
    }

    #[tokio::test]
    async fn stale_expected_version_is_rejected() {
        let (_temp, store) = test_store().await;
        let item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Update me".to_owned(),
                description: "Expect conflict".to_owned(),
                automation_claimable: true,
                agent_model_override: None,
                agent_reasoning_effort_override: None,
            },
        )
        .await
        .unwrap();

        let err = update_item(
            &store,
            "demo",
            item.id,
            UpdateWorkItem {
                title: Some("Changed".to_owned()),
                description: None,
                automation_claimable: None,
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                expect_version: Some(item.version + 1),
            },
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("version conflict"));
    }

    #[tokio::test]
    async fn delete_removes_item_from_lists() {
        let (_temp, store) = test_store().await;
        let item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Delete me".to_owned(),
                description: "Hide after deletion".to_owned(),
                automation_claimable: true,
                agent_model_override: None,
                agent_reasoning_effort_override: None,
            },
        )
        .await
        .unwrap();

        delete_item(&store, "demo", item.id).await.unwrap();

        assert!(list_items(&store, "demo", None).await.unwrap().is_empty());
        assert!(get_item(&store, "demo", item.id).await.is_err());
    }

    #[tokio::test]
    async fn claiming_item_records_agent_identity() {
        let (_temp, store) = test_store().await;
        let item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Claim me".to_owned(),
                description: "Available work".to_owned(),
                automation_claimable: true,
                agent_model_override: None,
                agent_reasoning_effort_override: None,
            },
        )
        .await
        .unwrap();

        let claimed = claim_item(&store, "demo", "agent-a", WorkState::Open)
            .await
            .unwrap()
            .unwrap();
        let comments = list_comments(&store, "demo", item.id).await.unwrap();
        let events = list_events(&store, "demo", Some(item.id), None)
            .await
            .unwrap();

        assert_eq!(claimed.id, item.id);
        assert_eq!(claimed.state, WorkState::InProgress);
        assert_eq!(claimed.version, item.version + 1);
        assert_eq!(claimed.claimed_by.as_deref(), Some("agent-a"));
        assert!(claimed.claimed_at.is_some());
        assert!(
            comments
                .iter()
                .any(|comment| comment.body == "Claimed by agent-a")
        );
        assert!(
            events
                .iter()
                .any(|event| event.event_type == "item_claimed")
        );
    }

    #[tokio::test]
    async fn claiming_is_atomic_for_racing_agents() {
        let (_temp, store) = test_store().await;
        create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Race item".to_owned(),
                description: "Only one agent can own this".to_owned(),
                automation_claimable: true,
                agent_model_override: None,
                agent_reasoning_effort_override: None,
            },
        )
        .await
        .unwrap();

        let (first, second) = tokio::join!(
            claim_item(&store, "demo", "agent-a", WorkState::Open),
            claim_item(&store, "demo", "agent-b", WorkState::Open)
        );
        let claims = [first.unwrap(), second.unwrap()];
        let in_progress = list_items(&store, "demo", Some(WorkState::InProgress))
            .await
            .unwrap();

        assert_eq!(claims.iter().filter(|claim| claim.is_some()).count(), 1);
        assert_eq!(in_progress.len(), 1);
        assert!(matches!(
            in_progress[0].claimed_by.as_deref(),
            Some("agent-a" | "agent-b")
        ));
    }

    #[tokio::test]
    async fn claim_respects_project_scope() {
        let (_temp, store) = test_store().await;
        create_item(
            &store,
            "other",
            CreateWorkItem {
                title: "Other item".to_owned(),
                description: "Should not be claimed from demo".to_owned(),
                automation_claimable: true,
                agent_model_override: None,
                agent_reasoning_effort_override: None,
            },
        )
        .await
        .unwrap();

        let claimed = claim_item(&store, "demo", "agent-a", WorkState::Open)
            .await
            .unwrap();

        assert!(claimed.is_none());
    }

    #[tokio::test]
    async fn unclaimable_item_is_skipped_until_enabled() {
        let (_temp, store) = test_store().await;
        let item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Draft item".to_owned(),
                description: "Hold this back from automation".to_owned(),
                automation_claimable: false,
                agent_model_override: None,
                agent_reasoning_effort_override: None,
            },
        )
        .await
        .unwrap();

        let skipped = claim_item(&store, "demo", "agent-a", WorkState::Open)
            .await
            .unwrap();
        assert!(skipped.is_none());

        let enabled = update_item(
            &store,
            "demo",
            item.id,
            UpdateWorkItem {
                title: None,
                description: None,
                automation_claimable: Some(true),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                expect_version: Some(item.version),
            },
        )
        .await
        .unwrap();
        let claimed = claim_item(&store, "demo", "agent-a", WorkState::Open)
            .await
            .unwrap()
            .unwrap();

        assert!(enabled.automation_claimable);
        assert_eq!(claimed.id, item.id);
        assert_eq!(claimed.claimed_by.as_deref(), Some("agent-a"));
    }

    #[tokio::test]
    async fn release_requires_current_claimant() {
        let (_temp, store) = test_store().await;
        let item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Owned item".to_owned(),
                description: "Only the claimant can release it".to_owned(),
                automation_claimable: true,
                agent_model_override: None,
                agent_reasoning_effort_override: None,
            },
        )
        .await
        .unwrap();
        claim_item(&store, "demo", "agent-a", WorkState::Open)
            .await
            .unwrap()
            .unwrap();

        let err = release_item(&store, "demo", item.id, "agent-b", None)
            .await
            .unwrap_err();

        assert!(err.to_string().contains("claimed by agent-a"));
    }

    #[tokio::test]
    async fn finish_moves_done_and_records_report() {
        let (_temp, store) = test_store().await;
        let item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Finish item".to_owned(),
                description: "Complete with report".to_owned(),
                automation_claimable: true,
                agent_model_override: None,
                agent_reasoning_effort_override: None,
            },
        )
        .await
        .unwrap();
        claim_item(&store, "demo", "agent-a", WorkState::Open)
            .await
            .unwrap()
            .unwrap();

        let finished = finish_item(&store, "demo", item.id, "agent-a", "Finished cleanly")
            .await
            .unwrap();
        let comments = list_comments(&store, "demo", item.id).await.unwrap();
        let events = list_events(&store, "demo", Some(item.id), None)
            .await
            .unwrap();

        assert_eq!(finished.state, WorkState::Done);
        assert_eq!(finished.claimed_by, None);
        assert!(finished.finished_at.is_some());
        assert!(
            comments
                .iter()
                .any(|comment| comment.body == "Finished cleanly")
        );
        assert!(
            events
                .iter()
                .any(|event| event.event_type == "item_finished")
        );
    }

    #[tokio::test]
    async fn stale_claim_recovery_releases_old_claim() {
        let (_temp, store) = test_store().await;
        let item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Stale item".to_owned(),
                description: "Claim should be recovered".to_owned(),
                automation_claimable: true,
                agent_model_override: None,
                agent_reasoning_effort_override: None,
            },
        )
        .await
        .unwrap();
        claim_item(&store, "demo", "agent-a", WorkState::Open)
            .await
            .unwrap()
            .unwrap();
        let project_id = projects::project_id(&store, "demo").await.unwrap();
        let mut model: WorkItemActiveModel = get_item_model(&store, project_id, item.id)
            .await
            .unwrap()
            .into();
        model.claimed_at = Set(Some(
            (OffsetDateTime::now_utc() - Duration::minutes(30))
                .format(&Rfc3339)
                .unwrap(),
        ));
        model.update(store.db().as_ref()).await.unwrap();

        let recovered = recover_stale_claims(&store, "demo", 10).await.unwrap();
        let item = get_item(&store, "demo", item.id).await.unwrap();

        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].agent_id, "agent-a");
        assert_eq!(item.state, WorkState::Open);
        assert_eq!(item.claimed_by, None);
    }
}
