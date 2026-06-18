use rootcause::{Result, prelude::*};
use sea_orm::{ActiveModelTrait, ActiveValue::Set, TransactionTrait};

use crate::{
    backend::{
        entities::work_item::WorkItemActiveModel,
        events, projects,
        storage::{Store, utc_now},
        work_item_comments, work_item_events, work_items,
    },
    shared::view_models::{AuthorType, CommentView},
};

#[derive(Clone, Debug)]
pub struct AddComment {
    pub author_type: AuthorType,
    pub author_name: Option<String>,
    pub body: String,
}

pub async fn add_comment(
    store: &Store,
    project_name: &str,
    item_id: i64,
    create: AddComment,
) -> Result<CommentView> {
    if create.body.trim().is_empty() {
        bail!("comment body cannot be empty");
    }
    let AddComment {
        author_type,
        author_name,
        body,
    } = create;

    let project_id = projects::project_id(store, project_name).await?;
    let txn = store
        .db()
        .begin()
        .await
        .context("failed to start comment add")?;
    let item = work_items::get(&txn, project_id, item_id).await?;
    let now = utc_now();

    let comment =
        work_item_comments::insert_in_tx(&txn, item_id, author_type, author_name, body.as_str())
            .await?;

    let mut item_active: WorkItemActiveModel = item.clone().into();
    item_active.version = Set(item.version + 1);
    item_active.updated_at = Set(now);
    item_active
        .update(&txn)
        .await
        .context("failed to update item after comment")?;

    work_item_events::record_event_in_tx(
        &txn,
        project_id,
        Some(item_id),
        "comment_added",
        "Added comment",
    )
    .await?;
    txn.commit().await.context("failed to commit comment add")?;
    events::publish_comment_changed(project_name, item_id);

    work_item_comments::to_view(comment)
}

pub async fn list_comments(
    store: &Store,
    project_name: &str,
    item_id: i64,
) -> Result<Vec<CommentView>> {
    let project_id = projects::project_id(store, project_name).await?;
    work_items::get(store.db().as_ref(), project_id, item_id).await?;

    work_item_comments::list_for_item(store.db().as_ref(), item_id)
        .await?
        .into_iter()
        .map(work_item_comments::to_view)
        .collect()
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::backend::{
        items::{CreateWorkItem, create_item, list_events},
        projects::{CreateProject, create_project},
    };

    async fn test_store() -> (TempDir, Store, i64) {
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
        let item = create_item(
            &store,
            "demo",
            CreateWorkItem {
                title: "Comment target".to_owned(),
                description: "Collect comments".to_owned(),
                state: "open".to_owned(),
                agent_model_override: None,
                agent_reasoning_effort_override: None,
                initial_labels: Vec::new(),
            },
        )
        .await
        .unwrap();
        (temp, store, item.id)
    }

    #[tokio::test]
    async fn comments_append_in_created_order() {
        let (_temp, store, item_id) = test_store().await;

        add_comment(
            &store,
            "demo",
            item_id,
            AddComment {
                author_type: AuthorType::User,
                author_name: Some("Lukas".to_owned()),
                body: "First".to_owned(),
            },
        )
        .await
        .unwrap();
        add_comment(
            &store,
            "demo",
            item_id,
            AddComment {
                author_type: AuthorType::Agent,
                author_name: Some("codex".to_owned()),
                body: "Second".to_owned(),
            },
        )
        .await
        .unwrap();

        let comments = list_comments(&store, "demo", item_id).await.unwrap();

        assert_eq!(comments.len(), 2);
        assert_eq!(comments[0].body, "First");
        assert_eq!(comments[1].author_type, AuthorType::Agent);

        let events = list_events(&store, "demo", Some(item_id), None)
            .await
            .unwrap();
        let comment_events = events
            .iter()
            .filter(|event| event.event_type == "comment_added")
            .collect::<Vec<_>>();

        assert_eq!(comment_events.len(), 2);
        assert!(comment_events.iter().all(|event| {
            event.work_item_id == Some(item_id)
                && event.body == "Added comment"
                && event.actor_type.is_none()
                && event.actor_id.is_none()
                && event.agent_run_id.is_none()
        }));
    }

    #[tokio::test]
    async fn missing_project_comment_is_rejected() {
        let (_temp, store, item_id) = test_store().await;

        let err = add_comment(
            &store,
            "missing",
            item_id,
            AddComment {
                author_type: AuthorType::User,
                author_name: None,
                body: "Nope".to_owned(),
            },
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("project 'missing' does not exist"));
    }
}
