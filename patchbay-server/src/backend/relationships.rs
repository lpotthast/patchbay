use std::collections::BTreeMap;

use rootcause::{Result, prelude::*};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, TransactionTrait};

use crate::{
    backend::{
        entities::{
            work_item::{self, WorkItem},
            work_item_relationship::WorkItemRelationshipModel,
        },
        events, projects,
        storage::Store,
        work_item_events, work_item_relationships, work_items,
    },
    shared::view_models::{
        DeleteWorkItemRelationshipResponse, WorkItemRelationshipDirection,
        WorkItemRelationshipItemSummary, WorkItemRelationshipListEntry, WorkItemRelationshipView,
    },
};

pub async fn list_item_relationships(
    store: &Store,
    project_name: &str,
    item_id: i64,
) -> Result<Vec<WorkItemRelationshipListEntry>> {
    let project_id = projects::project_id(store, project_name).await?;
    work_items::get(store.db().as_ref(), project_id, item_id).await?;
    let relationships =
        work_item_relationships::for_item(store.db().as_ref(), project_id, item_id).await?;
    let views = relationships_to_views(store, project_id, &relationships).await?;
    Ok(relationships
        .into_iter()
        .zip(views)
        .map(
            |(relationship, relationship_view)| WorkItemRelationshipListEntry {
                direction: direction_for_item(&relationship, item_id),
                relationship: relationship_view,
            },
        )
        .collect())
}

pub async fn create_relationship(
    store: &Store,
    project_name: &str,
    source_work_item_id: i64,
    target_work_item_id: i64,
    kind: String,
) -> Result<WorkItemRelationshipListEntry> {
    let kind = normalize_kind(kind)?;
    let project_id = projects::project_id(store, project_name).await?;
    let txn = store
        .db()
        .begin()
        .await
        .context("failed to start relationship create")?;
    let source = work_items::get(&txn, project_id, source_work_item_id).await?;
    ensure_not_self(source_work_item_id, target_work_item_id)?;
    let target = work_items::get(&txn, project_id, target_work_item_id).await?;
    ensure_no_duplicate(
        &txn,
        project_id,
        source_work_item_id,
        target_work_item_id,
        &kind,
        None,
    )
    .await?;

    let relationship = work_item_relationships::insert_in_tx(
        &txn,
        project_id,
        source_work_item_id,
        target_work_item_id,
        &kind,
    )
    .await?;
    let source = work_items::touch(&txn, source).await?;
    let target = work_items::touch(&txn, target).await?;
    record_relationship_event(
        &txn,
        project_id,
        source_work_item_id,
        "relationship_created",
        format!(
            "Created relationship #{} {} #{}",
            source_work_item_id, kind, target_work_item_id
        ),
    )
    .await?;
    record_relationship_event(
        &txn,
        project_id,
        target_work_item_id,
        "relationship_created",
        format!(
            "Created incoming relationship from #{}: {}",
            source_work_item_id, kind
        ),
    )
    .await?;
    txn.commit()
        .await
        .context("failed to commit relationship create")?;
    publish_touched_items(project_name, source.id, target.id);

    Ok(WorkItemRelationshipListEntry {
        relationship: relationship_to_view(store, relationship).await?,
        direction: WorkItemRelationshipDirection::Outgoing,
    })
}

pub async fn update_relationship(
    store: &Store,
    project_name: &str,
    relationship_id: i64,
    kind: String,
) -> Result<WorkItemRelationshipView> {
    update_relationship_inner(store, project_name, None, relationship_id, kind).await
}

pub async fn update_relationship_for_item(
    store: &Store,
    project_name: &str,
    item_id: i64,
    relationship_id: i64,
    kind: String,
) -> Result<WorkItemRelationshipView> {
    update_relationship_inner(store, project_name, Some(item_id), relationship_id, kind).await
}

pub async fn delete_relationship(
    store: &Store,
    project_name: &str,
    relationship_id: i64,
) -> Result<DeleteWorkItemRelationshipResponse> {
    delete_relationship_inner(store, project_name, None, relationship_id).await
}

pub async fn delete_relationship_for_item(
    store: &Store,
    project_name: &str,
    item_id: i64,
    relationship_id: i64,
) -> Result<DeleteWorkItemRelationshipResponse> {
    delete_relationship_inner(store, project_name, Some(item_id), relationship_id).await
}

async fn update_relationship_inner(
    store: &Store,
    project_name: &str,
    requested_item_id: Option<i64>,
    relationship_id: i64,
    kind: String,
) -> Result<WorkItemRelationshipView> {
    let kind = normalize_kind(kind)?;
    let project_id = projects::project_id(store, project_name).await?;
    let txn = store
        .db()
        .begin()
        .await
        .context("failed to start relationship update")?;
    if let Some(item_id) = requested_item_id {
        work_items::get(&txn, project_id, item_id).await?;
    }
    let relationship = work_item_relationships::get(&txn, project_id, relationship_id).await?;
    ensure_requested_item_touches_relationship(requested_item_id, &relationship)?;
    let source = work_items::get(&txn, project_id, relationship.source_work_item_id).await?;
    let target = work_items::get(&txn, project_id, relationship.target_work_item_id).await?;
    ensure_no_duplicate(
        &txn,
        project_id,
        relationship.source_work_item_id,
        relationship.target_work_item_id,
        &kind,
        Some(relationship_id),
    )
    .await?;

    let updated = work_item_relationships::update_kind_in_tx(&txn, relationship, &kind).await?;
    let source = work_items::touch(&txn, source).await?;
    let target = work_items::touch(&txn, target).await?;
    let body = format!("Updated relationship #{relationship_id} kind to {kind}");
    record_relationship_event(
        &txn,
        project_id,
        updated.source_work_item_id,
        "relationship_updated",
        body.clone(),
    )
    .await?;
    record_relationship_event(
        &txn,
        project_id,
        updated.target_work_item_id,
        "relationship_updated",
        body,
    )
    .await?;
    txn.commit()
        .await
        .context("failed to commit relationship update")?;
    publish_touched_items(project_name, source.id, target.id);

    relationship_to_view(store, updated).await
}

async fn delete_relationship_inner(
    store: &Store,
    project_name: &str,
    requested_item_id: Option<i64>,
    relationship_id: i64,
) -> Result<DeleteWorkItemRelationshipResponse> {
    let project_id = projects::project_id(store, project_name).await?;
    let txn = store
        .db()
        .begin()
        .await
        .context("failed to start relationship delete")?;
    if let Some(item_id) = requested_item_id {
        work_items::get(&txn, project_id, item_id).await?;
    }
    let relationship = work_item_relationships::get(&txn, project_id, relationship_id).await?;
    ensure_requested_item_touches_relationship(requested_item_id, &relationship)?;
    let source = work_items::get(&txn, project_id, relationship.source_work_item_id).await?;
    let target = work_items::get(&txn, project_id, relationship.target_work_item_id).await?;

    work_item_relationships::delete_by_id_in_tx(&txn, relationship_id).await?;
    let source = work_items::touch(&txn, source).await?;
    let target = work_items::touch(&txn, target).await?;
    let body = format!(
        "Deleted relationship #{}: #{} {} #{}",
        relationship_id,
        relationship.source_work_item_id,
        relationship.kind,
        relationship.target_work_item_id
    );
    record_relationship_event(
        &txn,
        project_id,
        relationship.source_work_item_id,
        "relationship_deleted",
        body.clone(),
    )
    .await?;
    record_relationship_event(
        &txn,
        project_id,
        relationship.target_work_item_id,
        "relationship_deleted",
        body,
    )
    .await?;
    txn.commit()
        .await
        .context("failed to commit relationship delete")?;
    publish_touched_items(project_name, source.id, target.id);
    let relationship_view = relationship_to_view(store, relationship).await?;

    Ok(DeleteWorkItemRelationshipResponse {
        deleted: true,
        relationship: relationship_view,
    })
}

fn normalize_kind(kind: String) -> Result<String> {
    let kind = kind.trim().to_owned();
    if kind.is_empty() {
        bail!("relationship kind cannot be empty");
    }
    Ok(kind)
}

fn ensure_not_self(source_work_item_id: i64, target_work_item_id: i64) -> Result<()> {
    if source_work_item_id == target_work_item_id {
        bail!("relationship source and target work items must differ");
    }
    Ok(())
}

async fn ensure_no_duplicate<C>(
    conn: &C,
    project_id: i64,
    source_work_item_id: i64,
    target_work_item_id: i64,
    kind: &str,
    except_relationship_id: Option<i64>,
) -> Result<()>
where
    C: sea_orm::ConnectionTrait,
{
    if work_item_relationships::exact_relationship_exists(
        conn,
        project_id,
        source_work_item_id,
        target_work_item_id,
        kind,
        except_relationship_id,
    )
    .await?
    {
        bail!(
            "duplicate relationship already exists for source item {source_work_item_id}, target item {target_work_item_id}, and kind '{kind}'"
        );
    }
    Ok(())
}

fn ensure_requested_item_touches_relationship(
    requested_item_id: Option<i64>,
    relationship: &WorkItemRelationshipModel,
) -> Result<()> {
    let Some(item_id) = requested_item_id else {
        return Ok(());
    };
    if relationship.source_work_item_id == item_id || relationship.target_work_item_id == item_id {
        return Ok(());
    }
    bail!(
        "relationship {} does not touch item {}",
        relationship.id,
        item_id
    )
}

fn direction_for_item(
    relationship: &WorkItemRelationshipModel,
    item_id: i64,
) -> WorkItemRelationshipDirection {
    if relationship.source_work_item_id == item_id {
        WorkItemRelationshipDirection::Outgoing
    } else {
        WorkItemRelationshipDirection::Incoming
    }
}

async fn relationship_to_view(
    store: &Store,
    relationship: WorkItemRelationshipModel,
) -> Result<WorkItemRelationshipView> {
    relationships_to_views(store, relationship.project_id, &[relationship])
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| report!("failed to build relationship view"))
}

async fn relationships_to_views(
    store: &Store,
    project_id: i64,
    relationships: &[WorkItemRelationshipModel],
) -> Result<Vec<WorkItemRelationshipView>> {
    if relationships.is_empty() {
        return Ok(Vec::new());
    }

    let mut item_ids = relationships
        .iter()
        .flat_map(|relationship| {
            [
                relationship.source_work_item_id,
                relationship.target_work_item_id,
            ]
        })
        .collect::<Vec<_>>();
    item_ids.sort_unstable();
    item_ids.dedup();

    let items = WorkItem::find()
        .filter(work_item::Column::ProjectId.eq(project_id))
        .filter(work_item::Column::Id.is_in(item_ids))
        .all(store.db().as_ref())
        .await
        .context("failed to load relationship item summaries")?;
    let summaries = work_items::models_to_views(store, project_id, items)
        .await?
        .into_iter()
        .map(|item| {
            (
                item.id,
                WorkItemRelationshipItemSummary {
                    id: item.id,
                    title: item.title,
                    state: item.state,
                    version: item.version,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();

    relationships
        .iter()
        .map(|relationship| {
            let source = summaries
                .get(&relationship.source_work_item_id)
                .cloned()
                .ok_or_else(|| {
                    report!(
                        "relationship {} references missing source item {}",
                        relationship.id,
                        relationship.source_work_item_id
                    )
                })?;
            let target = summaries
                .get(&relationship.target_work_item_id)
                .cloned()
                .ok_or_else(|| {
                    report!(
                        "relationship {} references missing target item {}",
                        relationship.id,
                        relationship.target_work_item_id
                    )
                })?;
            Ok(WorkItemRelationshipView {
                id: relationship.id,
                project_id: relationship.project_id,
                kind: relationship.kind.clone(),
                source_work_item_id: relationship.source_work_item_id,
                target_work_item_id: relationship.target_work_item_id,
                source,
                target,
                created_at: relationship.created_at.clone(),
                updated_at: relationship.updated_at.clone(),
            })
        })
        .collect()
}

async fn record_relationship_event<C>(
    conn: &C,
    project_id: i64,
    item_id: i64,
    event_type: &str,
    body: String,
) -> Result<()>
where
    C: sea_orm::ConnectionTrait,
{
    work_item_events::record_event_in_tx(conn, project_id, Some(item_id), event_type, &body)
        .await?;
    Ok(())
}

fn publish_touched_items(project_name: &str, source_work_item_id: i64, target_work_item_id: i64) {
    events::publish_work_item_changed(project_name, source_work_item_id);
    if target_work_item_id != source_work_item_id {
        events::publish_work_item_changed(project_name, target_work_item_id);
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::backend::{
        items::{CreateWorkItem, create_item, delete_item, get_item, list_events},
        projects::{CreateProject, create_project},
    };

    async fn test_store() -> (TempDir, Store, i64, i64) {
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
        let source = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Source".to_owned(),
                description: "Creates the relationship".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap();
        let target = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Target".to_owned(),
                description: "Receives the relationship".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap();
        (temp, store, source.id, target.id)
    }

    #[tokio::test]
    async fn relationships_list_incoming_and_outgoing_entries() {
        let (_temp, store, source_id, target_id) = test_store().await;

        let created = create_relationship(
            &store,
            "demo",
            source_id,
            target_id,
            " is follow-up of ".to_owned(),
        )
        .await
        .unwrap();

        assert_eq!(created.direction, WorkItemRelationshipDirection::Outgoing);
        assert_eq!(created.relationship.kind, "is follow-up of");
        assert_eq!(created.relationship.source.id, source_id);
        assert_eq!(created.relationship.target.id, target_id);

        let source_relationships = list_item_relationships(&store, "demo", source_id)
            .await
            .unwrap();
        assert_eq!(source_relationships.len(), 1);
        assert_eq!(
            source_relationships[0].direction,
            WorkItemRelationshipDirection::Outgoing
        );

        let target_relationships = list_item_relationships(&store, "demo", target_id)
            .await
            .unwrap();
        assert_eq!(target_relationships.len(), 1);
        assert_eq!(
            target_relationships[0].direction,
            WorkItemRelationshipDirection::Incoming
        );
    }

    #[tokio::test]
    async fn relationship_create_validates_self_empty_duplicate_and_project_scope() {
        let (temp, store, source_id, target_id) = test_store().await;

        let self_link = create_relationship(
            &store,
            "demo",
            source_id,
            source_id,
            "duplicates".to_owned(),
        )
        .await
        .unwrap_err();
        assert!(self_link.to_string().contains("must differ"));

        let empty_kind = create_relationship(&store, "demo", source_id, target_id, " ".to_owned())
            .await
            .unwrap_err();
        assert!(empty_kind.to_string().contains("kind cannot be empty"));

        create_relationship(&store, "demo", source_id, target_id, "relates".to_owned())
            .await
            .unwrap();
        let duplicate =
            create_relationship(&store, "demo", source_id, target_id, "relates".to_owned())
                .await
                .unwrap_err();
        assert!(duplicate.to_string().contains("duplicate relationship"));

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
        let other_item = create_item(
            &store,
            "other",
            CreateWorkItem {
                title: "Other".to_owned(),
                description: "Other project".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap();
        let cross_project = create_relationship(
            &store,
            "demo",
            source_id,
            other_item.id,
            "crosses".to_owned(),
        )
        .await
        .unwrap_err();
        assert!(
            cross_project
                .to_string()
                .contains("does not exist in this project")
        );
    }

    #[tokio::test]
    async fn relationship_update_and_delete_touch_both_items_and_emit_events() {
        let (_temp, store, source_id, target_id) = test_store().await;
        let before_source = get_item(&store, "demo", source_id).await.unwrap();
        let before_target = get_item(&store, "demo", target_id).await.unwrap();
        let created =
            create_relationship(&store, "demo", source_id, target_id, "blocks".to_owned())
                .await
                .unwrap();
        let relationship_id = created.relationship.id;

        let updated = update_relationship(&store, "demo", relationship_id, "unblocks".to_owned())
            .await
            .unwrap();
        assert_eq!(updated.kind, "unblocks");

        let after_update_source = get_item(&store, "demo", source_id).await.unwrap();
        let after_update_target = get_item(&store, "demo", target_id).await.unwrap();
        assert_eq!(after_update_source.version, before_source.version + 2);
        assert_eq!(after_update_target.version, before_target.version + 2);

        let deleted = delete_relationship(&store, "demo", relationship_id)
            .await
            .unwrap();
        assert!(deleted.deleted);
        assert!(
            list_item_relationships(&store, "demo", source_id)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            list_item_relationships(&store, "demo", target_id)
                .await
                .unwrap()
                .is_empty()
        );

        let source_events = list_events(&store, "demo", Some(source_id), None)
            .await
            .unwrap();
        let target_events = list_events(&store, "demo", Some(target_id), None)
            .await
            .unwrap();
        for event_type in [
            "relationship_created",
            "relationship_updated",
            "relationship_deleted",
        ] {
            assert!(
                source_events
                    .iter()
                    .any(|event| event.event_type == event_type),
                "missing source event {event_type}"
            );
            assert!(
                target_events
                    .iter()
                    .any(|event| event.event_type == event_type),
                "missing target event {event_type}"
            );
        }
    }

    #[tokio::test]
    async fn relationship_update_rejects_duplicate_kind() {
        let (_temp, store, source_id, target_id) = test_store().await;
        create_relationship(&store, "demo", source_id, target_id, "first".to_owned())
            .await
            .unwrap();
        let second = create_relationship(&store, "demo", source_id, target_id, "second".to_owned())
            .await
            .unwrap();

        let duplicate =
            update_relationship(&store, "demo", second.relationship.id, "first".to_owned())
                .await
                .unwrap_err();

        assert!(duplicate.to_string().contains("duplicate relationship"));
    }

    #[tokio::test]
    async fn deleting_work_item_cascades_relationships_without_orphans() {
        let (_temp, store, source_id, target_id) = test_store().await;
        create_relationship(&store, "demo", source_id, target_id, "blocks".to_owned())
            .await
            .unwrap();

        delete_item(&store, "demo", source_id).await.unwrap();

        let target_relationships = list_item_relationships(&store, "demo", target_id)
            .await
            .unwrap();
        assert!(target_relationships.is_empty());
    }
}
