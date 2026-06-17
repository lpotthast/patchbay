use crudkit_rs::prelude::*;
use crudkit_sea_orm::{CkField, CkId, CkSeaOrmBridge, CkSeaOrmCreateModel, CkSeaOrmUpdateModel};
use sea_orm::{DerivePrimaryKey, EntityTrait, EnumIter, PrimaryKeyTrait};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

pub type AgentRun = Entity;
pub type AgentRunModel = Model;
pub type AgentRunActiveModel = ActiveModel;
pub type AgentRunId = ModelId;

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
#[sea_orm(table_name = "agent_runs")]
#[read_view(table_name = "agent_runs_read_view")]
pub struct Model {
    #[sea_orm(primary_key)]
    #[serde(skip_deserializing)]
    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub id: i64,

    #[ck_update_model(exclude)]
    pub project_id: i64,

    pub work_item_id: Option<i64>,

    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub memory_event_id: Option<i64>,

    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub trigger_id: Option<i64>,

    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub trigger_name: Option<String>,

    pub mode: String,

    pub tool_name: String,

    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub status: String,

    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub command: String,

    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub working_dir: String,

    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub worktree_path: Option<String>,

    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub branch_name: Option<String>,

    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub process_id: Option<i64>,

    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub exit_code: Option<i64>,

    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub log_path: Option<String>,

    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub prompt_path: Option<String>,

    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub agent_model: Option<String>,

    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub agent_reasoning_effort: Option<String>,

    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub commit_required: bool,

    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub commit_outcome: String,

    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub commit_shas: String,

    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub pr_requested: bool,

    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub pr_url: Option<String>,

    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub cleanup_status: String,

    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub worktree_cleaned_at: Option<String>,

    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub result_summary: String,

    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub started_at: Option<String>,

    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub finished_at: Option<String>,

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
