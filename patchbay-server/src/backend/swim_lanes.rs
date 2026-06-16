use rootcause::{Result, prelude::*};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
};

use crate::{
    backend::{
        entities::swim_lane::{self, SwimLane, SwimLaneActiveModel, SwimLaneModel},
        projects,
        storage::{Store, utc_now},
    },
    shared::view_models::SwimLaneView,
};

const DEFAULT_SWIM_LANES: [(&str, &str, i64, bool); 4] = [
    ("idea", "Idea", 10, true),
    ("open", "Open", 20, true),
    ("in_progress", "In progress", 30, false),
    ("done", "Done", 40, false),
];

pub async fn list_swim_lanes(store: &Store, project_name: &str) -> Result<Vec<SwimLaneView>> {
    let project_id = projects::project_id(store, project_name).await?;
    list_swim_lanes_for_project_id(store, project_id).await
}

pub async fn list_swim_lanes_for_project_id(
    store: &Store,
    project_id: i64,
) -> Result<Vec<SwimLaneView>> {
    let lanes = SwimLane::find()
        .filter(swim_lane::Column::ProjectId.eq(project_id))
        .order_by_asc(swim_lane::Column::Position)
        .order_by_asc(swim_lane::Column::Id)
        .all(store.db().as_ref())
        .await
        .context("failed to list swim-lanes")?;

    Ok(lanes.into_iter().map(model_to_view).collect())
}

pub async fn ensure_default_swim_lanes_for_project_id(
    store: &Store,
    project_id: i64,
) -> Result<()> {
    ensure_default_swim_lanes_in_conn(store.db().as_ref(), project_id).await
}

pub(crate) async fn ensure_default_swim_lanes_in_conn<C>(conn: &C, project_id: i64) -> Result<()>
where
    C: sea_orm::ConnectionTrait,
{
    for (identifier, name, position, can_create_items) in DEFAULT_SWIM_LANES {
        if SwimLane::find()
            .filter(swim_lane::Column::ProjectId.eq(project_id))
            .filter(swim_lane::Column::Identifier.eq(identifier))
            .one(conn)
            .await
            .context_with(|| format!("failed to check swim-lane '{identifier}'"))?
            .is_some()
        {
            continue;
        }

        let now = utc_now();
        let active = SwimLaneActiveModel {
            project_id: Set(project_id),
            identifier: Set(identifier.to_owned()),
            name: Set(name.to_owned()),
            position: Set(position),
            can_create_items: Set(can_create_items),
            created_at: Set(now.clone()),
            updated_at: Set(now),
            ..Default::default()
        };
        active
            .insert(conn)
            .await
            .context_with(|| format!("failed to create swim-lane '{identifier}'"))?;
    }
    Ok(())
}

pub fn normalize_identifier(identifier: impl Into<String>) -> Result<String> {
    let identifier = identifier.into().trim().to_owned();
    if identifier.is_empty() {
        bail!("swim-lane identifier cannot be empty");
    }
    if identifier.contains('=') {
        bail!("swim-lane identifier cannot contain '='");
    }
    Ok(identifier)
}

pub fn normalize_name(name: impl Into<String>) -> Result<String> {
    let name = name.into().trim().to_owned();
    if name.is_empty() {
        bail!("swim-lane name cannot be empty");
    }
    Ok(name)
}

fn model_to_view(model: SwimLaneModel) -> SwimLaneView {
    SwimLaneView {
        id: model.id,
        project_id: model.project_id,
        identifier: model.identifier,
        name: model.name,
        position: model.position,
        can_create_items: model.can_create_items,
        created_at: model.created_at,
        updated_at: model.updated_at,
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::backend::projects::{CreateProject, create_project};

    #[tokio::test]
    async fn default_swim_lanes_configure_item_creation() {
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

        let flags = list_swim_lanes(&store, "demo")
            .await
            .unwrap()
            .into_iter()
            .map(|lane| (lane.identifier, lane.can_create_items))
            .collect::<Vec<_>>();

        assert_eq!(
            flags,
            vec![
                ("idea".to_owned(), true),
                ("open".to_owned(), true),
                ("in_progress".to_owned(), false),
                ("done".to_owned(), false),
            ]
        );
    }
}
