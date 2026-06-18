use crudkit_rs::prelude::*;
use crudkit_sea_orm::{CkField, CkId, CkSeaOrmBridge, CkSeaOrmCreateModel, CkSeaOrmUpdateModel};
use sea_orm::{DerivePrimaryKey, EntityTrait, EnumIter, PrimaryKeyTrait};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

pub type Project = Entity;
pub type ProjectModel = Model;
pub type ProjectActiveModel = ActiveModel;
pub type ProjectId = ModelId;

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
#[sea_orm(table_name = "projects")]
#[read_view(table_name = "projects_read_view")]
pub struct Model {
    #[sea_orm(primary_key)]
    #[serde(skip_deserializing)]
    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub id: i64,

    #[ck_update_model(exclude)]
    pub name: String,

    pub display_name: String,

    pub path: Option<String>,

    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub path_exists: bool,

    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub path_checked_at: Option<String>,

    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub system_prompt: String,

    pub memory: String,

    #[ck_create_model(exclude)]
    pub workspace_mode: String,

    #[ck_create_model(exclude)]
    pub max_code_edit_agents: i64,

    #[ck_create_model(exclude)]
    pub max_read_only_agents: i64,

    #[ck_create_model(exclude)]
    pub create_pr: bool,

    #[ck_create_model(exclude)]
    pub auto_commit: bool,

    #[ck_create_model(exclude)]
    pub commit_standard: String,

    #[ck_create_model(exclude)]
    pub revert_strategy: String,

    #[ck_create_model(exclude)]
    pub stale_claim_minutes: i64,

    #[ck_create_model(exclude)]
    pub worktree_cleanup_policy: String,

    #[ck_create_model(exclude)]
    pub default_agent_tool: String,

    pub default_agent_model: Option<String>,

    pub default_agent_reasoning_effort: Option<String>,

    #[ck_create_model(exclude)]
    pub agent_sandbox_mode: String,

    #[ck_create_model(exclude)]
    pub agent_extra_writable_roots: String,

    #[ck_create_model(exclude)]
    pub agent_git_command_policy: String,

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
