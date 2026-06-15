use sea_orm::{DerivePrimaryKey, EntityTrait, EnumIter, PrimaryKeyTrait};
use serde::{Deserialize, Serialize};

pub type WorkItemEventActiveModel = ActiveModel;

#[derive(Clone, Debug, PartialEq, Eq, sea_orm::DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "work_item_events")]
pub struct Model {
    #[sea_orm(primary_key)]
    #[serde(skip_deserializing)]
    pub id: i64,

    pub project_id: i64,

    pub work_item_id: Option<i64>,

    pub event_type: String,

    pub body: String,

    pub actor_type: Option<String>,

    pub actor_id: Option<String>,

    pub agent_run_id: Option<i64>,

    pub created_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, sea_orm::DeriveRelation)]
pub enum Relation {}

impl sea_orm::ActiveModelBehavior for ActiveModel {}
