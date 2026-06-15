use crudkit_rs::prelude::*;
use crudkit_sea_orm::{CkField, CkId, CkSeaOrmBridge, CkSeaOrmCreateModel, CkSeaOrmUpdateModel};
use sea_orm::{DerivePrimaryKey, EntityTrait, EnumIter, PrimaryKeyTrait};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

pub type WorkItem = Entity;
pub type WorkItemModel = Model;
pub type WorkItemActiveModel = ActiveModel;
pub type WorkItemId = ModelId;

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
    ToSchema,
    Serialize,
    Deserialize,
)]
#[sea_orm(table_name = "work_items")]
pub struct Model {
    #[sea_orm(primary_key)]
    #[serde(skip_deserializing)]
    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub id: i64,

    #[ck_update_model(exclude)]
    pub project_id: i64,

    pub title: String,

    pub description: String,

    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub claimed_by: Option<String>,

    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub claimed_at: Option<String>,

    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub claim_expires_at: Option<String>,

    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub finished_at: Option<String>,

    pub agent_model_override: Option<String>,

    pub agent_reasoning_effort_override: Option<String>,

    #[ck_create_model(exclude)]
    #[ck_update_model(exclude)]
    pub version: i64,

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

pub mod read_view {
    use crudkit_sea_orm::{CkField, CkId, CkSeaOrmBridge};
    use sea_orm::{DerivePrimaryKey, EntityTrait, EnumIter, PrimaryKeyTrait};
    use serde::{Deserialize, Serialize};
    use utoipa::ToSchema;

    #[derive(
        Clone,
        Debug,
        PartialEq,
        Eq,
        sea_orm::DeriveEntityModel,
        CkId,
        CkField,
        CkSeaOrmBridge,
        ToSchema,
        Serialize,
        Deserialize,
    )]
    #[sea_orm(table_name = "work_items_read_view")]
    pub struct Model {
        #[sea_orm(primary_key)]
        #[serde(skip_deserializing)]
        pub id: i64,

        pub project_id: i64,

        pub title: String,

        pub description: String,

        pub claimed_by: Option<String>,

        pub claimed_at: Option<String>,

        pub claim_expires_at: Option<String>,

        pub finished_at: Option<String>,

        pub agent_model_override: Option<String>,

        pub agent_reasoning_effort_override: Option<String>,

        pub version: i64,

        pub created_at: String,

        pub updated_at: String,

        pub state_label: Option<String>,

        pub has_validation_errors: bool,
    }

    #[derive(Debug, Clone, Copy, EnumIter, sea_orm::DeriveRelation)]
    pub enum Relation {}

    impl sea_orm::ActiveModelBehavior for ActiveModel {}
}
