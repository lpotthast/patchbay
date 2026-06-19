use rootcause::{Result, prelude::*};
use sea_orm::{ActiveModelTrait, ActiveValue::Set, TransactionTrait};

use crate::{
    backend::{
        agent_ids, events, projects,
        storage::{Store, utc_now},
        work_item_comments, work_item_events, work_items, workflow_labels,
    },
    shared::view_models::{CommentView, WorkItemView},
};

use super::active_claims;

pub(crate) async fn progress_item(
    store: &Store,
    project_name: &str,
    item_id: i64,
    agent_id: &str,
    body: &str,
) -> Result<CommentView> {
    agent_ids::validate_agent_id(agent_id)?;
    if body.trim().is_empty() {
        bail!("progress body cannot be empty");
    }

    let project_id = projects::project_id(store, project_name).await?;
    let txn = store
        .db()
        .begin()
        .await
        .context("failed to start item progress")?;
    let claim = active_claims::load_in_tx(&txn, project_id, item_id, agent_id).await?;

    let comment = work_item_comments::insert_agent_in_tx(&txn, item_id, agent_id, body).await?;

    let item_active = claim.touch_active_model(utc_now());
    item_active
        .update(&txn)
        .await
        .context("failed to update item after progress")?;
    work_item_events::record_event_in_tx(&txn, project_id, Some(item_id), "progress_added", body)
        .await?;
    txn.commit()
        .await
        .context("failed to commit item progress")?;
    events::publish_comment_changed(project_name, item_id);

    work_item_comments::to_view(comment)
}

pub(crate) async fn finish_item(
    store: &Store,
    project_name: &str,
    item_id: i64,
    agent_id: &str,
    report: &str,
) -> Result<WorkItemView> {
    agent_ids::validate_agent_id(agent_id)?;
    if report.trim().is_empty() {
        bail!("finish report cannot be empty");
    }

    let project_id = projects::project_id(store, project_name).await?;
    let txn = store
        .db()
        .begin()
        .await
        .context("failed to start item finish")?;
    let claim = active_claims::load_in_tx(&txn, project_id, item_id, agent_id).await?;

    let now = utc_now();
    work_item_comments::insert_agent_in_tx(&txn, item_id, agent_id, report).await?;

    let mut active = claim.clear_active_model(now.clone());
    active.finished_at = Set(Some(now.clone()));
    let updated = active
        .update(&txn)
        .await
        .context("failed to finish work item")?;
    workflow_labels::apply_plan_in_tx(
        &txn,
        project_id,
        item_id,
        workflow_labels::finish_workflow_label_plan(),
    )
    .await?;
    work_item_events::record_event_in_tx(&txn, project_id, Some(item_id), "item_finished", report)
        .await?;
    txn.commit().await.context("failed to commit item finish")?;
    events::publish_work_item_changed(project_name, item_id);

    work_items::model_to_view(store, updated).await
}
