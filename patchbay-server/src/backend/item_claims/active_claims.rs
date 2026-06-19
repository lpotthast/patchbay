use rootcause::{Result, prelude::*};
use sea_orm::ConnectionTrait;

use crate::backend::{
    entities::work_item::{WorkItemActiveModel, WorkItemModel},
    work_items,
};

#[derive(Debug)]
pub(crate) struct ActiveClaim {
    item: WorkItemModel,
}

impl ActiveClaim {
    pub(crate) fn touch_active_model(&self, updated_at: String) -> WorkItemActiveModel {
        work_items::touch_active_model(self.item.clone(), updated_at)
    }

    pub(crate) fn clear_active_model(self, updated_at: String) -> WorkItemActiveModel {
        work_items::clear_claim_active_model(self.item, updated_at)
    }
}

pub(crate) async fn load_in_tx<C>(
    conn: &C,
    project_id: i64,
    item_id: i64,
    agent_id: &str,
) -> Result<ActiveClaim>
where
    C: ConnectionTrait,
{
    let item = work_items::get(conn, project_id, item_id).await?;
    ensure_active_claim(&item, agent_id)?;
    Ok(ActiveClaim { item })
}

fn ensure_active_claim(item: &WorkItemModel, agent_id: &str) -> Result<()> {
    match item.claimed_by.as_deref() {
        Some(claimed_by) if claimed_by == agent_id => Ok(()),
        Some(claimed_by) => bail!("item {} is claimed by {claimed_by}", item.id),
        None => bail!("item {} is not claimed", item.id),
    }
}
