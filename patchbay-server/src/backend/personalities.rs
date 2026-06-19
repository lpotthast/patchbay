use rootcause::{Result, prelude::*};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter,
    QueryOrder, QuerySelect,
};

use crate::{
    backend::{
        entities::{
            automation_trigger,
            personality::{self, Personality, PersonalityActiveModel, PersonalityModel},
        },
        projects,
        storage::{Store, utc_now},
    },
    shared::view_models::PersonalityView,
};

pub(crate) const DEFAULT_PERSONALITY_NAME: &str = "Default";

impl From<PersonalityModel> for PersonalityView {
    fn from(personality: PersonalityModel) -> Self {
        Self {
            id: personality.id,
            project_id: personality.project_id,
            name: personality.name,
            personality_description: personality.personality_description,
            created_at: personality.created_at,
            updated_at: personality.updated_at,
        }
    }
}

pub(crate) async fn list_personalities(
    store: &Store,
    project_name: &str,
) -> Result<Vec<PersonalityView>> {
    let project_id = projects::project_id(store, project_name).await?;
    let personalities = Personality::find()
        .filter(personality::Column::ProjectId.eq(project_id))
        .order_by_asc(personality::Column::Name)
        .all(store.db().as_ref())
        .await
        .context("failed to list personalities")?;
    Ok(personalities.into_iter().map(Into::into).collect())
}

pub(crate) fn normalize_name(name: String) -> Result<String> {
    let name = name.trim().to_owned();
    if name.is_empty() {
        bail!("personality name cannot be empty");
    }
    Ok(name)
}

pub(crate) fn normalize_description(description: String) -> String {
    description
}

pub(crate) async fn ensure_default_personality_in_conn<C>(
    conn: &C,
    project_id: i64,
) -> Result<PersonalityModel>
where
    C: ConnectionTrait,
{
    if let Some(existing) = Personality::find()
        .filter(personality::Column::ProjectId.eq(project_id))
        .filter(personality::Column::Name.eq(DEFAULT_PERSONALITY_NAME))
        .limit(1)
        .one(conn)
        .await
        .context("failed to load default personality")?
    {
        return Ok(existing);
    }

    let now = utc_now();
    Ok(PersonalityActiveModel {
        project_id: Set(project_id),
        name: Set(DEFAULT_PERSONALITY_NAME.to_owned()),
        personality_description: Set(String::new()),
        created_at: Set(now.clone()),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(conn)
    .await
    .context("failed to create default personality")?)
}

pub(crate) async fn ensure_default_personality_for_project_id(
    store: &Store,
    project_id: i64,
) -> Result<PersonalityView> {
    Ok(
        ensure_default_personality_in_conn(store.db().as_ref(), project_id)
            .await?
            .into(),
    )
}

pub(crate) async fn default_personality_id_in_conn<C>(conn: &C, project_id: i64) -> Result<i64>
where
    C: ConnectionTrait,
{
    Ok(ensure_default_personality_in_conn(conn, project_id)
        .await?
        .id)
}

pub(crate) async fn default_personality_id(store: &Store, project_id: i64) -> Result<i64> {
    default_personality_id_in_conn(store.db().as_ref(), project_id).await
}

pub(crate) async fn validate_personality_for_project(
    store: &Store,
    project_id: i64,
    personality_id: i64,
) -> Result<PersonalityModel> {
    Personality::find_by_id(personality_id)
        .filter(personality::Column::ProjectId.eq(project_id))
        .one(store.db().as_ref())
        .await
        .context("failed to load personality")?
        .ok_or_else(|| report!("personality {personality_id} does not exist in this project"))
}

pub(crate) async fn personality_description_for_prompt(
    store: &Store,
    project_id: i64,
    personality_id: Option<i64>,
) -> Result<Option<String>> {
    let Some(personality_id) = personality_id else {
        return Ok(None);
    };
    let personality = validate_personality_for_project(store, project_id, personality_id).await?;
    let description = personality.personality_description;
    Ok((!description.trim().is_empty()).then_some(description))
}

pub(crate) async fn ensure_personality_name_available(
    store: &Store,
    project_id: i64,
    name: &str,
    except_id: Option<i64>,
) -> Result<()> {
    let mut query = Personality::find()
        .filter(personality::Column::ProjectId.eq(project_id))
        .filter(personality::Column::Name.eq(name));
    if let Some(except_id) = except_id {
        query = query.filter(personality::Column::Id.ne(except_id));
    }
    let exists = query
        .limit(1)
        .one(store.db().as_ref())
        .await
        .context("failed to check personality name")?
        .is_some();
    if exists {
        bail!("personality name '{name}' already exists in this project");
    }
    Ok(())
}

pub(crate) async fn validate_personality_delete(
    store: &Store,
    model: &PersonalityModel,
) -> Result<()> {
    if model.name == DEFAULT_PERSONALITY_NAME {
        bail!("the Default personality cannot be deleted");
    }
    let referencing = automation_trigger::Entity::find()
        .filter(automation_trigger::Column::ProjectId.eq(model.project_id))
        .filter(automation_trigger::Column::PersonalityId.eq(model.id))
        .limit(1)
        .one(store.db().as_ref())
        .await
        .context("failed to check personality references")?;
    if let Some(trigger) = referencing {
        bail!(
            "personality '{}' is referenced by automation trigger '{}'",
            model.name,
            trigger.name
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use sea_orm::{ActiveModelTrait, ActiveValue::Set};
    use tempfile::TempDir;

    use super::*;
    use crate::{
        backend::{
            entities::automation_trigger,
            projects::{CreateProject, create_project},
        },
        shared::view_models::{AutomationActivation, AutomationEffect, AutomationRunMutability},
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
    async fn new_project_gets_empty_default_personality() {
        let (_temp, store) = test_store().await;

        let personalities = list_personalities(&store, "demo").await.unwrap();

        assert_eq!(personalities.len(), 1);
        assert_eq!(personalities[0].name, DEFAULT_PERSONALITY_NAME);
        assert_eq!(personalities[0].personality_description, "");
    }

    #[tokio::test]
    async fn validation_rejects_cross_project_personality() {
        let (temp, store) = test_store().await;
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
        let demo_project_id = projects::project_id(&store, "demo").await.unwrap();
        let other_default = list_personalities(&store, "other").await.unwrap()[0].id;

        let err = validate_personality_for_project(&store, demo_project_id, other_default)
            .await
            .unwrap_err();

        assert!(err.to_string().contains("does not exist in this project"));
    }

    #[tokio::test]
    async fn delete_rejects_default_and_referenced_personality() {
        let (_temp, store) = test_store().await;
        let project_id = projects::project_id(&store, "demo").await.unwrap();
        let default = ensure_default_personality_in_conn(store.db().as_ref(), project_id)
            .await
            .unwrap();
        let default_err = validate_personality_delete(&store, &default)
            .await
            .unwrap_err();
        assert!(default_err.to_string().contains("cannot be deleted"));

        let now = utc_now();
        let custom = PersonalityActiveModel {
            project_id: Set(project_id),
            name: Set("Review".to_owned()),
            personality_description: Set("Review carefully.".to_owned()),
            created_at: Set(now.clone()),
            updated_at: Set(now.clone()),
            ..Default::default()
        }
        .insert(store.db().as_ref())
        .await
        .unwrap();
        automation_trigger::ActiveModel {
            project_id: Set(project_id),
            name: Set("Review work".to_owned()),
            enabled: Set(true),
            activation: Set(AutomationActivation::WorkItem.as_storage().to_owned()),
            effect: Set(AutomationEffect::ConsumeWork.as_storage().to_owned()),
            schedule: Set("@every 15s".to_owned()),
            tool_name: Set("codex".to_owned()),
            mutability: Set(AutomationRunMutability::ReadOnly.as_storage().to_owned()),
            personality_id: Set(Some(custom.id)),
            prompt: Set(String::new()),
            work_item_selector: Set(Some(
                r#"{"All":[{"column_name":"state","operator":"=","value":{"String":"open"}}]}"#
                    .to_owned(),
            )),
            priority: Set(0),
            evaluation_count: Set(0),
            pending_evaluation_count: Set(0),
            last_evaluation_queued_at: Set(None),
            last_evaluated_at: Set(None),
            next_evaluation_at: Set(None),
            last_event_id: Set(None),
            created_at: Set(now.clone()),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(store.db().as_ref())
        .await
        .unwrap();

        let err = validate_personality_delete(&store, &custom)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("referenced by automation trigger"));
    }
}
