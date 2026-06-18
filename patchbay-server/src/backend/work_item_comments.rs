use std::{collections::BTreeMap, str::FromStr};

use rootcause::{Result, prelude::*};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
    Statement,
};

use crate::{
    backend::{
        entities::comment::{self, Comment, CommentActiveModel, CommentModel},
        storage::utc_now,
    },
    shared::view_models::{AuthorType, CommentView},
};

pub(crate) async fn insert_in_tx<C>(
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
    Ok(active
        .insert(conn)
        .await
        .context("failed to add item comment")?)
}

pub(crate) async fn list_for_item<C>(conn: &C, item_id: i64) -> Result<Vec<CommentModel>>
where
    C: sea_orm::ConnectionTrait,
{
    Ok(Comment::find()
        .filter(comment::Column::WorkItemId.eq(item_id))
        .order_by_asc(comment::Column::CreatedAt)
        .order_by_asc(comment::Column::Id)
        .all(conn)
        .await
        .context("failed to list comments")?)
}

pub(crate) async fn counts_for_items<C>(conn: &C, item_ids: &[i64]) -> Result<BTreeMap<i64, i64>>
where
    C: sea_orm::ConnectionTrait,
{
    if item_ids.is_empty() {
        return Ok(BTreeMap::new());
    }

    let placeholders = (1..=item_ids.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let values = item_ids.iter().copied().map(Into::into).collect::<Vec<_>>();
    let rows = conn
        .query_all(Statement::from_sql_and_values(
            sea_orm::DbBackend::Sqlite,
            format!(
                r#"
                SELECT work_item_id, COUNT(*) AS count
                FROM comments
                WHERE work_item_id IN ({placeholders})
                GROUP BY work_item_id
                "#
            ),
            values,
        ))
        .await
        .context("failed to count item comments")?;

    let mut counts = BTreeMap::new();
    for row in rows {
        counts.insert(
            row.try_get::<i64>("", "work_item_id")
                .context("failed to read comment count item id")?,
            row.try_get::<i64>("", "count")
                .context("failed to read item comment count")?,
        );
    }
    Ok(counts)
}

pub(crate) fn to_view(comment: CommentModel) -> Result<CommentView> {
    Ok(CommentView {
        id: comment.id,
        work_item_id: comment.work_item_id,
        author_type: AuthorType::from_str(&comment.author_type)?,
        author_name: comment.author_name,
        body: comment.body,
        created_at: comment.created_at,
    })
}
