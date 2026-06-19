use crudkit_core::condition::Condition;
use rootcause::{Result, prelude::*};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter,
    QueryOrder, Statement, TransactionTrait,
};

use crate::{
    backend::{
        entities::{
            work_item::{self, WorkItem, WorkItemActiveModel},
            work_item_event,
        },
        events, item_labels, label_conditions, projects,
        storage::{Store, utc_now},
        work_item_events, work_item_labels, work_item_relationships,
        work_item_updates::{self, WorkItemUpdatePlan},
        work_items, workflow_labels,
    },
    shared::view_models::{
        AgentReasoningEffort, CreateWorkItemLabelRequest, STATE_LABEL_KEY, WorkItemEventView,
        WorkItemView,
    },
};

pub use work_item_updates::UpdateWorkItem;

#[derive(Clone, Debug)]
pub struct CreateWorkItem {
    pub title: String,
    pub description: String,
    pub state: String,
    pub agent_model_override: Option<String>,
    pub agent_reasoning_effort_override: Option<AgentReasoningEffort>,
    pub initial_labels: Vec<CreateWorkItemLabelRequest>,
}

pub async fn list_items(
    store: &Store,
    project_name: &str,
    state: Option<String>,
) -> Result<Vec<WorkItemView>> {
    let project_id = projects::project_id(store, project_name).await?;
    let item_ids = match state {
        Some(state) => {
            let state = workflow_labels::normalize_state_value(state)?;
            let ids =
                work_item_labels::item_ids_with_state(store.db().as_ref(), project_id, &state)
                    .await?;
            if ids.is_empty() {
                return Ok(Vec::new());
            }
            Some(ids)
        }
        None => None,
    };
    let mut query = WorkItem::find()
        .filter(work_item::Column::ProjectId.eq(project_id))
        .order_by_desc(work_item::Column::UpdatedAt)
        .order_by_desc(work_item::Column::Id);

    if let Some(item_ids) = item_ids {
        query = query.filter(work_item::Column::Id.is_in(item_ids));
    }

    let items = query
        .all(store.db().as_ref())
        .await
        .context("failed to list work items")?;
    work_items::models_to_views(store, project_id, items).await
}

pub async fn count_items_outside_work_item_states(
    store: &Store,
    project_name: &str,
) -> Result<i64> {
    let project_id = projects::project_id(store, project_name).await?;
    let row = store
        .db()
        .query_one(Statement::from_sql_and_values(
            sea_orm::DbBackend::Sqlite,
            r#"
            SELECT COUNT(*) AS count
            FROM work_items AS wi
            WHERE wi.project_id = ?1
              AND NOT EXISTS (
                  SELECT 1
                  FROM work_item_labels AS wil
                  JOIN work_item_states AS wis
                    ON wis.project_id = wi.project_id
                   AND wis.identifier = wil.label_value
                  WHERE wil.project_id = wi.project_id
                    AND wil.work_item_id = wi.id
                    AND wil.label_key = ?2
              )
            "#,
            vec![project_id.into(), STATE_LABEL_KEY.to_owned().into()],
        ))
        .await
        .context("failed to count work items outside authored states")?;

    row.map(|row| row.try_get::<i64>("", "count"))
        .transpose()
        .context("failed to read work items outside authored states count")?
        .ok_or_else(|| report!("missing work items outside authored states count"))
}

pub async fn item_matches_condition(
    store: &Store,
    project_name: &str,
    item_id: i64,
    condition: &Condition,
) -> Result<bool> {
    let selector = label_conditions::ValidatedLabelCondition::new(condition)?;
    let project_id = projects::project_id(store, project_name).await?;
    let item = work_items::get(store.db().as_ref(), project_id, item_id).await?;
    let labels = work_item_labels::for_item(store.db().as_ref(), project_id, item.id).await?;
    Ok(selector.matches_automation_selector(&labels))
}

pub async fn get_item(store: &Store, project_name: &str, item_id: i64) -> Result<WorkItemView> {
    let project_id = projects::project_id(store, project_name).await?;
    let item = work_items::get(store.db().as_ref(), project_id, item_id).await?;
    work_items::model_to_view(store, item).await
}

pub async fn create_item(
    store: &Store,
    project_name: &str,
    create: CreateWorkItem,
) -> Result<WorkItemView> {
    work_item_updates::validate_item_text(&create.title, &create.description)?;
    let state_label = workflow_labels::normalize_state_value(create.state)?;
    let agent_model_override = projects::normalize_optional(create.agent_model_override);
    let initial_labels = item_labels::normalize_initial_labels(
        create
            .initial_labels
            .into_iter()
            .map(|label| (label.key, label.value)),
    )
    .context("invalid initial labels")?;

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
    workflow_labels::apply_plan_in_tx(
        &txn,
        project_id,
        item.id,
        workflow_labels::state_workflow_label_plan(&state_label),
    )
    .await?;
    for label in &initial_labels {
        work_item_labels::insert_in_tx(
            &txn,
            project_id,
            item.id,
            &label.key,
            label.value.as_deref(),
        )
        .await?;
    }
    work_item_events::record_event_in_tx(
        &txn,
        project_id,
        Some(item.id),
        "item_created",
        "Created item",
    )
    .await?;
    txn.commit().await.context("failed to commit item create")?;
    events::publish_work_item_changed(project_name, item.id);

    work_items::model_to_view(store, item).await
}

pub async fn update_item(
    store: &Store,
    project_name: &str,
    item_id: i64,
    update: UpdateWorkItem,
) -> Result<WorkItemView> {
    let update = WorkItemUpdatePlan::new(update)?;

    let project_id = projects::project_id(store, project_name).await?;
    let txn = store
        .db()
        .begin()
        .await
        .context("failed to start item update")?;
    let existing = work_items::get(&txn, project_id, item_id).await?;
    work_items::check_expected_version(update.expect_version(), existing.version)?;
    let applied = update.apply_to(existing)?;

    let updated = applied
        .active
        .update(&txn)
        .await
        .context("failed to update work item")?;
    if applied.record_item_updated_event {
        work_item_events::record_event_in_tx(
            &txn,
            project_id,
            Some(item_id),
            "item_updated",
            "Updated item",
        )
        .await?;
    }
    if let Some(state) = applied.state {
        workflow_labels::apply_plan_in_tx(
            &txn,
            project_id,
            item_id,
            workflow_labels::state_workflow_label_plan(&state),
        )
        .await?;
        let event_body = workflow_labels::state_move_event_body(&state);
        work_item_events::record_event_in_tx(
            &txn,
            project_id,
            Some(item_id),
            "item_moved",
            &event_body,
        )
        .await?;
    }
    txn.commit().await.context("failed to commit item update")?;
    events::publish_work_item_changed(project_name, item_id);

    work_items::model_to_view(store, updated).await
}

pub async fn move_item(
    store: &Store,
    project_name: &str,
    item_id: i64,
    state: String,
    expect_version: Option<i64>,
) -> Result<WorkItemView> {
    update_item(
        store,
        project_name,
        item_id,
        UpdateWorkItem {
            title: None,
            description: None,
            state: Some(state),
            agent_model_override: None,
            agent_reasoning_effort_override: None,
            expect_version,
        },
    )
    .await
}

pub async fn delete_item(store: &Store, project_name: &str, item_id: i64) -> Result<()> {
    let project_id = projects::project_id(store, project_name).await?;
    let txn = store
        .db()
        .begin()
        .await
        .context("failed to start item delete")?;
    work_items::get(&txn, project_id, item_id).await?;
    let related_item_ids =
        work_item_relationships::related_item_ids_for_item(&txn, project_id, item_id).await?;

    work_item_events::record_event_in_tx(
        &txn,
        project_id,
        Some(item_id),
        "item_deleted",
        "Deleted item",
    )
    .await?;
    for related_item_id in &related_item_ids {
        work_item_events::record_event_in_tx(
            &txn,
            project_id,
            Some(*related_item_id),
            "relationship_deleted",
            &format!("Deleted relationships touching removed item #{item_id}"),
        )
        .await?;
    }
    WorkItem::delete_by_id(item_id)
        .exec(&txn)
        .await
        .context("failed to delete work item")?;
    txn.commit().await.context("failed to commit item delete")?;
    events::publish_work_item_changed(project_name, item_id);
    for related_item_id in related_item_ids {
        events::publish_work_item_changed(project_name, related_item_id);
    }
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
        work_items::get(store.db().as_ref(), project_id, item_id).await?;
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

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::backend::{
        comments::{AddComment, add_comment},
        item_label_service::add_label,
        projects::{CreateProject, create_project},
    };
    use crate::shared::view_models::AuthorType;

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
        create_project(
            &store,
            CreateProject {
                name: "other".to_owned(),
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
    async fn work_items_are_scoped_to_project() {
        let (_temp, store) = test_store().await;
        let demo_item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Demo item".to_owned(),
                description: "Build the demo item".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
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
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
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
    async fn creating_item_records_item_created_event_row() {
        let (_temp, store) = test_store().await;
        let project_id = projects::project_id(&store, "demo").await.unwrap();

        let item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Record event".to_owned(),
                description: "Persist the creation event".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap();

        let events = list_events(&store, "demo", Some(item.id), None)
            .await
            .unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].project_id, project_id);
        assert_eq!(events[0].work_item_id, Some(item.id));
        assert_eq!(events[0].event_type, "item_created");
        assert_eq!(events[0].body, "Created item");
        assert!(events[0].actor_type.is_none());
        assert!(events[0].actor_id.is_none());
        assert!(events[0].agent_run_id.is_none());
    }

    #[tokio::test]
    async fn creating_item_with_initial_labels_persists_normalized_labels() {
        let (_temp, store) = test_store().await;

        let item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Labeled create".to_owned(),
                description: "Initial labels should be visible immediately".to_owned(),
                state: " open ".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: vec![
                    CreateWorkItemLabelRequest {
                        key: " type ".to_owned(),
                        value: Some(" feature ".to_owned()),
                    },
                    CreateWorkItemLabelRequest {
                        key: "needs-verification".to_owned(),
                        value: Some("  ".to_owned()),
                    },
                ],
            },
        )
        .await
        .unwrap();

        assert_eq!(item.state.as_deref(), Some("open"));
        assert!(item.labels.iter().any(|label| {
            label.key == STATE_LABEL_KEY && label.value.as_deref() == Some("open")
        }));
        assert!(
            item.labels
                .iter()
                .any(|label| { label.key == "type" && label.value.as_deref() == Some("feature") })
        );
        assert!(
            item.labels
                .iter()
                .any(|label| label.key == "needs-verification" && label.value.is_none())
        );

        let listed = list_items(&store, "demo", None).await.unwrap();
        let listed_item = listed
            .iter()
            .find(|candidate| candidate.id == item.id)
            .unwrap();
        assert!(
            listed_item
                .labels
                .iter()
                .any(|label| { label.key == "type" && label.value.as_deref() == Some("feature") })
        );
        assert!(
            listed_item
                .labels
                .iter()
                .any(|label| label.key == "needs-verification" && label.value.is_none())
        );
    }

    #[tokio::test]
    async fn duplicate_initial_labels_reject_create_without_partial_item() {
        let (_temp, store) = test_store().await;

        let err = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Duplicate labels".to_owned(),
                description: "Should not create anything".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: vec![
                    CreateWorkItemLabelRequest {
                        key: " area ".to_owned(),
                        value: Some("frontend".to_owned()),
                    },
                    CreateWorkItemLabelRequest {
                        key: "area".to_owned(),
                        value: Some("backend".to_owned()),
                    },
                ],
            },
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("duplicate initial label key"));
        assert!(list_items(&store, "demo", None).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn invalid_initial_label_key_rejects_create_without_partial_item() {
        let (_temp, store) = test_store().await;

        let err = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Invalid label".to_owned(),
                description: "Should not create anything".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: vec![CreateWorkItemLabelRequest {
                    key: "bad=key".to_owned(),
                    value: Some("value".to_owned()),
                }],
            },
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("label key cannot contain '='"));
        assert!(list_items(&store, "demo", None).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn state_initial_label_rejects_create_without_partial_item() {
        let (_temp, store) = test_store().await;

        let err = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "State collision".to_owned(),
                description: "State belongs to the selector".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: vec![CreateWorkItemLabelRequest {
                    key: STATE_LABEL_KEY.to_owned(),
                    value: Some("review".to_owned()),
                }],
            },
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("use the state selector"));
        assert!(list_items(&store, "demo", None).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn blank_state_selector_rejects_create_without_partial_item() {
        let (_temp, store) = test_store().await;

        let err = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Blank state".to_owned(),
                description: "State cannot be blank".to_owned(),
                state: "  ".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: vec![CreateWorkItemLabelRequest {
                    key: "type".to_owned(),
                    value: Some("feature".to_owned()),
                }],
            },
        )
        .await
        .unwrap_err();

        assert!(
            err.to_string()
                .contains("state label value cannot be empty")
        );
        assert!(list_items(&store, "demo", None).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn list_items_hydrates_labels_state_and_comment_counts() {
        let (_temp, store) = test_store().await;
        let first = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "First item".to_owned(),
                description: "Has several comments and labels".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap();
        let second = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Second item".to_owned(),
                description: "Has an independent label and comment count".to_owned(),
                state: "ready".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap();
        add_label(
            &store,
            "demo",
            first.id,
            "severity".to_owned(),
            Some("high".to_owned()),
            None,
        )
        .await
        .unwrap();
        add_label(&store, "demo", second.id, "bug".to_owned(), None, None)
            .await
            .unwrap();

        for body in ["First comment", "Second comment"] {
            add_comment(
                &store,
                "demo",
                first.id,
                AddComment {
                    author_type: AuthorType::User,
                    author_name: Some("operator".to_owned()),
                    body: body.to_owned(),
                },
            )
            .await
            .unwrap();
        }
        add_comment(
            &store,
            "demo",
            second.id,
            AddComment {
                author_type: AuthorType::Agent,
                author_name: Some("agent-a".to_owned()),
                body: "Only comment".to_owned(),
            },
        )
        .await
        .unwrap();

        let items = list_items(&store, "demo", None).await.unwrap();
        let first = items.iter().find(|item| item.id == first.id).unwrap();
        let second = items.iter().find(|item| item.id == second.id).unwrap();

        assert_eq!(first.state.as_deref(), Some("open"));
        assert_eq!(first.comment_count, 2);
        assert!(
            first
                .labels
                .iter()
                .any(|label| { label.key == "severity" && label.value.as_deref() == Some("high") })
        );
        assert_eq!(second.state.as_deref(), Some("ready"));
        assert_eq!(second.comment_count, 1);
        assert!(
            second
                .labels
                .iter()
                .any(|label| { label.key == "bug" && label.value.is_none() })
        );
    }

    #[tokio::test]
    async fn counts_items_outside_authored_work_item_states() {
        let (_temp, store) = test_store().await;
        create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Valid item".to_owned(),
                description: "Uses an authored state".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap();
        create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Invalid item".to_owned(),
                description: "Uses an unconfigured state".to_owned(),
                state: "needs_triage".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap();
        let unlabeled = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Unlabeled item".to_owned(),
                description: "Has no state label".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap();
        create_item(
            &store,
            "other",
            CreateWorkItem {
                title: "Other project invalid item".to_owned(),
                description: "Should not affect the demo count".to_owned(),
                state: "needs_triage".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap();
        let project_id = projects::project_id(&store, "demo").await.unwrap();
        work_item_labels::delete_by_key_in_tx(
            store.db().as_ref(),
            project_id,
            unlabeled.id,
            STATE_LABEL_KEY,
        )
        .await
        .unwrap();

        let demo_count = count_items_outside_work_item_states(&store, "demo")
            .await
            .unwrap();
        let other_count = count_items_outside_work_item_states(&store, "other")
            .await
            .unwrap();

        assert_eq!(demo_count, 2);
        assert_eq!(other_count, 1);
    }

    #[tokio::test]
    async fn work_items_read_view_exposes_state_label() {
        let (_temp, store) = test_store().await;
        let item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Visible state".to_owned(),
                description: "Shows state in the CrudKit read view".to_owned(),
                state: "needs_triage".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap();
        let project_id = projects::project_id(&store, "demo").await.unwrap();

        let row = store
            .db()
            .query_one(Statement::from_sql_and_values(
                sea_orm::DbBackend::Sqlite,
                r#"
                SELECT "state_label"
                FROM "work_items_read_view"
                WHERE "project_id" = ?1
                  AND "id" = ?2
                "#,
                vec![project_id.into(), item.id.into()],
            ))
            .await
            .unwrap()
            .unwrap();
        let state_label = row.try_get::<Option<String>>("", "state_label").unwrap();

        assert_eq!(state_label.as_deref(), Some("needs_triage"));
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
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap();

        let moved = move_item(
            &store,
            "demo",
            item.id,
            "in_progress".to_owned(),
            Some(item.version),
        )
        .await
        .unwrap();

        assert_eq!(moved.state.as_deref(), Some("in_progress"));
        assert_eq!(moved.version, item.version + 1);
    }

    #[tokio::test]
    async fn updating_item_fields_and_state_is_one_versioned_change() {
        let (_temp, store) = test_store().await;
        let item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Update me".to_owned(),
                description: "Move and edit together".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap();

        let updated = update_item(
            &store,
            "demo",
            item.id,
            UpdateWorkItem {
                title: Some("Updated title".to_owned()),
                description: None,
                state: Some("review".to_owned()),
                agent_model_override: Some(Some("gpt-5.4".to_owned())),
                agent_reasoning_effort_override: None,
                expect_version: Some(item.version),
            },
        )
        .await
        .unwrap();
        let events = list_events(&store, "demo", Some(item.id), None)
            .await
            .unwrap();

        assert_eq!(updated.title, "Updated title");
        assert_eq!(updated.state.as_deref(), Some("review"));
        assert_eq!(updated.agent_model_override.as_deref(), Some("gpt-5.4"));
        assert_eq!(updated.version, item.version + 1);
        assert_eq!(
            events
                .iter()
                .filter(|event| event.event_type == "item_updated")
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| event.event_type == "item_moved")
                .count(),
            1
        );
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
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
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
                state: None,
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
    async fn empty_update_is_rejected_without_touching_item() {
        let (_temp, store) = test_store().await;
        let item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "No change".to_owned(),
                description: "Empty updates should not bump versions".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap();

        let err = update_item(&store, "demo", item.id, UpdateWorkItem::default())
            .await
            .unwrap_err();
        let unchanged = get_item(&store, "demo", item.id).await.unwrap();

        assert!(err.to_string().contains("requires at least one field"));
        assert_eq!(unchanged.version, item.version);
        assert_eq!(unchanged.updated_at, item.updated_at);
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
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap();

        delete_item(&store, "demo", item.id).await.unwrap();

        assert!(list_items(&store, "demo", None).await.unwrap().is_empty());
        assert!(get_item(&store, "demo", item.id).await.is_err());
    }
}
