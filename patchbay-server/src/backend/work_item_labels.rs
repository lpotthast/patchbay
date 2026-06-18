use std::collections::BTreeMap;

use rootcause::{Result, prelude::*};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
};

use crate::{
    backend::{
        entities::work_item_label::{
            self, WorkItemLabel, WorkItemLabelActiveModel, WorkItemLabelModel,
        },
        item_labels,
        storage::utc_now,
    },
    shared::view_models::{STATE_LABEL_KEY, WorkItemLabelView},
};

pub(crate) async fn item_ids_with_state<C>(
    conn: &C,
    project_id: i64,
    state: &str,
) -> Result<Vec<i64>>
where
    C: sea_orm::ConnectionTrait,
{
    let labels = WorkItemLabel::find()
        .filter(work_item_label::Column::ProjectId.eq(project_id))
        .filter(work_item_label::Column::Key.eq(STATE_LABEL_KEY))
        .filter(work_item_label::Column::Value.eq(state))
        .all(conn)
        .await
        .context_with(|| format!("failed to list items with state label '{state}'"))?;
    Ok(labels.into_iter().map(|label| label.work_item_id).collect())
}

pub(crate) async fn insert_in_tx<C>(
    conn: &C,
    project_id: i64,
    item_id: i64,
    key: &str,
    value: Option<&str>,
) -> Result<WorkItemLabelModel>
where
    C: sea_orm::ConnectionTrait,
{
    let now = utc_now();
    let active = WorkItemLabelActiveModel {
        project_id: Set(project_id),
        work_item_id: Set(item_id),
        key: Set(key.to_owned()),
        value: Set(value.map(ToOwned::to_owned)),
        created_at: Set(now.clone()),
        updated_at: Set(now),
        ..Default::default()
    };
    Ok(active.insert(conn).await.context_with(|| {
        format!(
            "failed to add label '{}'",
            item_labels::format_label(key, value)
        )
    })?)
}

pub(crate) async fn upsert_in_tx<C>(
    conn: &C,
    project_id: i64,
    item_id: i64,
    key: &str,
    value: Option<&str>,
) -> Result<WorkItemLabelModel>
where
    C: sea_orm::ConnectionTrait,
{
    if let Some(existing) = WorkItemLabel::find()
        .filter(work_item_label::Column::ProjectId.eq(project_id))
        .filter(work_item_label::Column::WorkItemId.eq(item_id))
        .filter(work_item_label::Column::Key.eq(key))
        .one(conn)
        .await
        .context_with(|| format!("failed to load label '{key}'"))?
    {
        let mut active: WorkItemLabelActiveModel = existing.into();
        active.value = Set(value.map(ToOwned::to_owned));
        active.updated_at = Set(utc_now());
        Ok(active
            .update(conn)
            .await
            .context_with(|| format!("failed to update label '{key}'"))?)
    } else {
        insert_in_tx(conn, project_id, item_id, key, value).await
    }
}

pub(crate) async fn delete_by_key_in_tx<C>(
    conn: &C,
    project_id: i64,
    item_id: i64,
    key: &str,
) -> Result<()>
where
    C: sea_orm::ConnectionTrait,
{
    WorkItemLabel::delete_many()
        .filter(work_item_label::Column::ProjectId.eq(project_id))
        .filter(work_item_label::Column::WorkItemId.eq(item_id))
        .filter(work_item_label::Column::Key.eq(key))
        .exec(conn)
        .await
        .context_with(|| format!("failed to delete label '{key}'"))?;
    Ok(())
}

pub(crate) async fn for_item<C>(
    conn: &C,
    project_id: i64,
    item_id: i64,
) -> Result<Vec<WorkItemLabelView>>
where
    C: sea_orm::ConnectionTrait,
{
    let labels = WorkItemLabel::find()
        .filter(work_item_label::Column::ProjectId.eq(project_id))
        .filter(work_item_label::Column::WorkItemId.eq(item_id))
        .order_by_asc(work_item_label::Column::Key)
        .order_by_asc(work_item_label::Column::Value)
        .all(conn)
        .await
        .context("failed to list item labels")?;
    Ok(labels.into_iter().map(to_view).collect())
}

pub(crate) async fn for_items<C>(
    conn: &C,
    project_id: i64,
    item_ids: &[i64],
) -> Result<BTreeMap<i64, Vec<WorkItemLabelView>>>
where
    C: sea_orm::ConnectionTrait,
{
    let labels = WorkItemLabel::find()
        .filter(work_item_label::Column::ProjectId.eq(project_id))
        .filter(work_item_label::Column::WorkItemId.is_in(item_ids.iter().copied()))
        .order_by_asc(work_item_label::Column::WorkItemId)
        .order_by_asc(work_item_label::Column::Key)
        .order_by_asc(work_item_label::Column::Value)
        .all(conn)
        .await
        .context("failed to list item labels")?;

    let mut labels_by_item = BTreeMap::<i64, Vec<WorkItemLabelView>>::new();
    for label in labels {
        labels_by_item
            .entry(label.work_item_id)
            .or_default()
            .push(to_view(label));
    }
    Ok(labels_by_item)
}

pub(crate) fn to_view(label: WorkItemLabelModel) -> WorkItemLabelView {
    WorkItemLabelView {
        id: label.id,
        project_id: label.project_id,
        work_item_id: label.work_item_id,
        key: label.key,
        value: label.value,
        created_at: label.created_at,
        updated_at: label.updated_at,
    }
}
