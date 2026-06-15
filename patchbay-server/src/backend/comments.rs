use std::str::FromStr;

use anyhow::{Context, Result, bail};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
    TransactionTrait,
};

use crate::{
    backend::{
        entities::{
            comment::{self, Comment, CommentActiveModel, CommentModel},
            work_item::WorkItemActiveModel,
        },
        events, items, projects,
        storage::{Store, utc_now},
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

    let project_id = projects::project_id(store, project_name).await?;
    let txn = store
        .db()
        .begin()
        .await
        .context("failed to start comment add")?;
    let item = items::get_item_model_in_tx(&txn, project_id, item_id).await?;
    let now = utc_now();

    let active = CommentActiveModel {
        work_item_id: Set(item_id),
        author_type: Set(create.author_type.as_storage().to_owned()),
        author_name: Set(create.author_name),
        body: Set(create.body),
        created_at: Set(now.clone()),
        ..Default::default()
    };
    let comment = active.insert(&txn).await.context("failed to add comment")?;

    let mut item_active: WorkItemActiveModel = item.clone().into();
    item_active.version = Set(item.version + 1);
    item_active.updated_at = Set(now);
    item_active
        .update(&txn)
        .await
        .context("failed to update item after comment")?;

    items::record_event_in_tx(
        &txn,
        project_id,
        Some(item_id),
        "comment_added",
        "Added comment",
    )
    .await?;
    txn.commit().await.context("failed to commit comment add")?;
    events::publish_comment_changed(project_name, item_id);

    model_to_view(comment)
}

pub async fn list_comments(
    store: &Store,
    project_name: &str,
    item_id: i64,
) -> Result<Vec<CommentView>> {
    let project_id = projects::project_id(store, project_name).await?;
    items::get_item_model(store, project_id, item_id).await?;

    let comments = Comment::find()
        .filter(comment::Column::WorkItemId.eq(item_id))
        .order_by_asc(comment::Column::CreatedAt)
        .order_by_asc(comment::Column::Id)
        .all(store.db().as_ref())
        .await
        .context("failed to list comments")?;

    comments.into_iter().map(model_to_view).collect()
}

fn model_to_view(comment: CommentModel) -> Result<CommentView> {
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
        items::{CreateWorkItem, create_item},
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
