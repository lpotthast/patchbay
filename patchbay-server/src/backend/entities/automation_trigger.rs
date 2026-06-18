use crudkit_rs::prelude::*;
use crudkit_sea_orm::{CkField, CkId, CkSeaOrmBridge, CkSeaOrmCreateModel, CkSeaOrmUpdateModel};
use sea_orm::{DerivePrimaryKey, EntityTrait, EnumIter, PrimaryKeyTrait};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

pub type AutomationTrigger = Entity;
pub type AutomationTriggerModel = Model;
pub type AutomationTriggerActiveModel = ActiveModel;
pub type AutomationTriggerId = ModelId;

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    sea_orm::DeriveEntityModel,
    CkId,
    CkField,
    CkSeaOrmBridge,
    CkSeaOrmCreateModel,
    CkSeaOrmUpdateModel,
    crudkit_sea_orm::ReadView,
    ToSchema,
    Serialize,
    Deserialize,
)]
#[sea_orm(table_name = "automation_triggers")]
#[read_view(table_name = "automation_triggers_read_view")]
pub struct Model {
    #[sea_orm(primary_key)]
    #[serde(skip_deserializing)]
    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub id: i64,

    #[ck_update_model(exclude)]
    pub project_id: i64,

    pub name: String,

    pub enabled: bool,

    #[serde(alias = "trigger_kind")]
    pub activation: String,

    pub effect: String,

    pub schedule: String,

    pub tool_name: String,

    pub prompt: String,

    pub work_item_selector: Option<String>,

    pub priority: i64,

    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub evaluation_count: i64,

    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub pending_evaluation_count: i64,

    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub last_evaluation_queued_at: Option<String>,

    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub last_evaluated_at: Option<String>,

    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub next_evaluation_at: Option<String>,

    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub last_event_id: Option<i64>,

    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub created_at: String,

    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, sea_orm::DeriveRelation)]
pub enum Relation {}

impl sea_orm::ActiveModelBehavior for ActiveModel {}
