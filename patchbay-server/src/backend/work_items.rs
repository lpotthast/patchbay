use std::collections::BTreeMap;

use rootcause::{Result, prelude::*};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter,
};

use crate::{
    backend::{
        agent_ids,
        entities::{
            agent_run::{self, AgentRun, AgentRunModel},
            work_item::{self, WorkItem, WorkItemActiveModel, WorkItemModel},
        },
        projects,
        storage::{Store, utc_now},
        work_item_comments, work_item_labels, workflow_labels,
    },
    shared::view_models::{
        AgentReasoningEffort, WorkItemClaimSourceView, WorkItemLabelView, WorkItemView,
    },
};

pub(crate) async fn get<C>(conn: &C, project_id: i64, item_id: i64) -> Result<WorkItemModel>
where
    C: ConnectionTrait,
{
    WorkItem::find_by_id(item_id)
        .filter(work_item::Column::ProjectId.eq(project_id))
        .one(conn)
        .await
        .context_with(|| format!("failed to load item {item_id}"))?
        .ok_or_else(|| report!("item {item_id} does not exist in this project"))
}

pub(crate) async fn touch<C>(conn: &C, item: WorkItemModel) -> Result<WorkItemModel>
where
    C: ConnectionTrait,
{
    let version = item.version;
    let mut active: WorkItemActiveModel = item.into();
    active.version = Set(version + 1);
    active.updated_at = Set(utc_now());
    Ok(active
        .update(conn)
        .await
        .context("failed to update item version")?)
}

pub(crate) fn check_expected_version(expected: Option<i64>, actual: i64) -> Result<()> {
    if let Some(expected) = expected
        && expected != actual
    {
        bail!("version conflict: expected {expected}, found {actual}");
    }
    Ok(())
}

pub(crate) async fn models_to_views(
    store: &Store,
    project_id: i64,
    items: Vec<WorkItemModel>,
) -> Result<Vec<WorkItemView>> {
    if items.is_empty() {
        return Ok(Vec::new());
    }

    let item_ids = items.iter().map(|item| item.id).collect::<Vec<_>>();
    let mut labels =
        work_item_labels::for_items(store.db().as_ref(), project_id, &item_ids).await?;
    let mut comment_counts =
        work_item_comments::counts_for_items(store.db().as_ref(), &item_ids).await?;
    let mut claim_sources =
        claim_sources_for_items(store.db().as_ref(), project_id, &items).await?;

    let mut views = Vec::with_capacity(items.len());
    for item in items {
        let item_id = item.id;
        views.push(to_view(
            item,
            labels.remove(&item_id).unwrap_or_default(),
            comment_counts.remove(&item_id).unwrap_or(0),
            claim_sources.remove(&item_id),
        )?);
    }
    Ok(views)
}

pub(crate) async fn model_to_view(store: &Store, item: WorkItemModel) -> Result<WorkItemView> {
    let labels = work_item_labels::for_item(store.db().as_ref(), item.project_id, item.id).await?;
    let comment_count = work_item_comments::counts_for_items(store.db().as_ref(), &[item.id])
        .await?
        .remove(&item.id)
        .unwrap_or(0);
    let mut claim_sources = claim_sources_for_items(
        store.db().as_ref(),
        item.project_id,
        std::slice::from_ref(&item),
    )
    .await?;
    let claim_source = claim_sources.remove(&item.id);
    to_view(item, labels, comment_count, claim_source)
}

async fn claim_sources_for_items<C>(
    conn: &C,
    project_id: i64,
    items: &[WorkItemModel],
) -> Result<BTreeMap<i64, WorkItemClaimSourceView>>
where
    C: ConnectionTrait,
{
    let run_to_item = items
        .iter()
        .filter_map(|item| {
            let run_id = agent_ids::parse_patchbay_run_agent_id(item.claimed_by.as_deref()?)?;
            Some((run_id, item.id))
        })
        .collect::<BTreeMap<_, _>>();
    if run_to_item.is_empty() {
        return Ok(BTreeMap::new());
    }

    let run_ids = run_to_item.keys().copied().collect::<Vec<_>>();
    let runs = AgentRun::find()
        .filter(agent_run::Column::ProjectId.eq(project_id))
        .filter(agent_run::Column::Id.is_in(run_ids))
        .all(conn)
        .await
        .context("failed to list claimed item agent runs")?;

    let mut claim_sources = BTreeMap::new();
    for run in runs {
        let Some(item_id) = run_to_item.get(&run.id).copied() else {
            continue;
        };
        if run.work_item_id != Some(item_id) {
            continue;
        }
        claim_sources.insert(item_id, claim_source_from_run(run));
    }

    Ok(claim_sources)
}

fn claim_source_from_run(run: AgentRunModel) -> WorkItemClaimSourceView {
    WorkItemClaimSourceView {
        run_id: run.id,
        trigger_id: run.trigger_id,
        trigger_name: projects::normalize_optional(run.trigger_name),
    }
}

fn to_view(
    item: WorkItemModel,
    labels: Vec<WorkItemLabelView>,
    comment_count: i64,
    claim_source: Option<WorkItemClaimSourceView>,
) -> Result<WorkItemView> {
    let state = workflow_labels::current_state(&labels);

    Ok(WorkItemView {
        id: item.id,
        project_id: item.project_id,
        title: item.title,
        description: item.description,
        state,
        labels,
        version: item.version,
        claimed_by: item.claimed_by,
        claimed_at: item.claimed_at,
        claim_expires_at: item.claim_expires_at,
        claim_source,
        finished_at: item.finished_at,
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
