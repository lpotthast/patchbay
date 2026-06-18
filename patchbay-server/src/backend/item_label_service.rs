use rootcause::{Result, prelude::*};
use sea_orm::TransactionTrait;

use crate::{
    backend::{
        events, item_labels, projects, storage::Store, work_item_events, work_item_labels,
        work_items,
    },
    shared::view_models::{
        DeleteWorkItemLabelResponse, ProjectLabelView, STATE_LABEL_KEY, WorkItemLabelView,
        WorkItemView,
    },
};

pub async fn list_item_labels(
    store: &Store,
    project_name: &str,
    item_id: i64,
) -> Result<Vec<WorkItemLabelView>> {
    let project_id = projects::project_id(store, project_name).await?;
    work_items::get(store.db().as_ref(), project_id, item_id).await?;
    work_item_labels::for_item(store.db().as_ref(), project_id, item_id).await
}

pub async fn list_project_labels(
    store: &Store,
    project_name: &str,
) -> Result<Vec<ProjectLabelView>> {
    let project_id = projects::project_id(store, project_name).await?;
    work_item_labels::project_label_summaries(store.db().as_ref(), project_id).await
}

pub async fn add_label(
    store: &Store,
    project_name: &str,
    item_id: i64,
    key: String,
    value: Option<String>,
    expect_version: Option<i64>,
) -> Result<WorkItemView> {
    let key = item_labels::normalize_key(key)?;
    let value = item_labels::normalize_value(value);
    item_labels::validate_pair(&key, value.as_deref())?;
    let project_id = projects::project_id(store, project_name).await?;
    let txn = store
        .db()
        .begin()
        .await
        .context("failed to start label add")?;
    let item = work_items::get(&txn, project_id, item_id).await?;
    work_items::check_expected_version(expect_version, item.version)?;
    if work_item_labels::item_has_key(&txn, project_id, item_id, &key, None).await? {
        bail!("item already has label '{key}'");
    }

    work_item_labels::insert_in_tx(&txn, project_id, item_id, &key, value.as_deref()).await?;
    let updated = work_items::touch(&txn, item).await?;
    let body = format!(
        "Added label {}",
        item_labels::format_label(&key, value.as_deref())
    );
    work_item_events::record_event_in_tx(&txn, project_id, Some(item_id), "label_added", &body)
        .await?;
    txn.commit().await.context("failed to commit label add")?;
    events::publish_work_item_changed(project_name, item_id);
    work_items::model_to_view(store, updated).await
}

pub async fn update_label(
    store: &Store,
    project_name: &str,
    item_id: i64,
    label_id: i64,
    key: Option<String>,
    value: Option<Option<String>>,
    expect_version: Option<i64>,
) -> Result<WorkItemView> {
    if key.is_none() && value.is_none() {
        bail!("label update requires at least one field");
    }

    let project_id = projects::project_id(store, project_name).await?;
    let txn = store
        .db()
        .begin()
        .await
        .context("failed to start label update")?;
    let item = work_items::get(&txn, project_id, item_id).await?;
    work_items::check_expected_version(expect_version, item.version)?;
    let existing = work_item_labels::get_for_item(&txn, project_id, item_id, label_id).await?;
    let key = match key {
        Some(key) => item_labels::normalize_key(key)?,
        None => existing.key.clone(),
    };
    let value = match value {
        Some(value) => item_labels::normalize_value(value),
        None => existing.value.clone(),
    };
    item_labels::validate_pair(&key, value.as_deref())?;
    if work_item_labels::item_has_key(&txn, project_id, item_id, &key, Some(label_id)).await? {
        bail!("item already has label '{key}'");
    }

    work_item_labels::update_in_tx(&txn, existing, key.clone(), value.clone()).await?;
    let updated = work_items::touch(&txn, item).await?;
    let body = format!(
        "Updated label {}",
        item_labels::format_label(&key, value.as_deref())
    );
    work_item_events::record_event_in_tx(&txn, project_id, Some(item_id), "label_updated", &body)
        .await?;
    txn.commit()
        .await
        .context("failed to commit label update")?;
    events::publish_work_item_changed(project_name, item_id);
    work_items::model_to_view(store, updated).await
}

pub async fn delete_label(
    store: &Store,
    project_name: &str,
    item_id: i64,
    label_id: i64,
    expect_version: Option<i64>,
) -> Result<DeleteWorkItemLabelResponse> {
    let project_id = projects::project_id(store, project_name).await?;
    let txn = store
        .db()
        .begin()
        .await
        .context("failed to start label delete")?;
    let item = work_items::get(&txn, project_id, item_id).await?;
    work_items::check_expected_version(expect_version, item.version)?;
    let label = work_item_labels::get_for_item(&txn, project_id, item_id, label_id).await?;
    if label.key == STATE_LABEL_KEY {
        bail!("state label cannot be deleted; move the item to another state instead");
    }
    let body = format!(
        "Deleted label {}",
        item_labels::format_label(&label.key, label.value.as_deref())
    );
    work_item_labels::delete_by_id_in_tx(&txn, label_id).await?;
    let updated = work_items::touch(&txn, item).await?;
    work_item_events::record_event_in_tx(&txn, project_id, Some(item_id), "label_deleted", &body)
        .await?;
    txn.commit()
        .await
        .context("failed to commit label delete")?;
    events::publish_work_item_changed(project_name, item_id);
    let work_item = work_items::model_to_view(store, updated).await?;
    Ok(DeleteWorkItemLabelResponse {
        deleted: true,
        label_id,
        work_item,
    })
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::backend::{
        items::{CreateWorkItem, create_item},
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
        (temp, store)
    }

    #[tokio::test]
    async fn add_update_and_delete_label_touch_item_and_preserve_state_label() {
        let (_temp, store) = test_store().await;
        let item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Label item".to_owned(),
                description: "Exercise label service behavior".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap();

        let added = add_label(
            &store,
            "demo",
            item.id,
            "priority".to_owned(),
            Some("high".to_owned()),
            Some(item.version),
        )
        .await
        .unwrap();
        let label_id = added
            .labels
            .iter()
            .find(|label| label.key == "priority")
            .unwrap()
            .id;

        let updated = update_label(
            &store,
            "demo",
            item.id,
            label_id,
            None,
            Some(Some("low".to_owned())),
            Some(added.version),
        )
        .await
        .unwrap();
        let deleted = delete_label(&store, "demo", item.id, label_id, Some(updated.version))
            .await
            .unwrap();

        assert_eq!(added.version, item.version + 1);
        assert_eq!(updated.version, added.version + 1);
        assert_eq!(deleted.work_item.version, updated.version + 1);
        assert!(deleted.deleted);
        assert_eq!(deleted.label_id, label_id);
        assert_eq!(deleted.work_item.state.as_deref(), Some("open"));
        assert!(
            !deleted
                .work_item
                .labels
                .iter()
                .any(|label| label.key == "priority")
        );
    }
}
