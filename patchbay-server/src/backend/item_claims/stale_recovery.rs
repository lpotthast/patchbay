use rootcause::{Result, prelude::*};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
    backend::{
        entities::work_item::{self, WorkItem},
        projects,
        storage::Store,
    },
    shared::view_models::RecoveredClaimView,
};

use super::claim_returns::{self, ReleaseAutomationDisposition};

pub(crate) async fn recover_stale_claims(
    store: &Store,
    project_name: &str,
    stale_after_minutes: i64,
) -> Result<Vec<RecoveredClaimView>> {
    if stale_after_minutes <= 0 {
        return Ok(Vec::new());
    }

    let project_id = projects::project_id(store, project_name).await?;
    let items = WorkItem::find()
        .filter(work_item::Column::ProjectId.eq(project_id))
        .filter(work_item::Column::ClaimedBy.is_not_null())
        .all(store.db().as_ref())
        .await
        .context("failed to list claimed work items")?;
    let cutoff = OffsetDateTime::now_utc() - Duration::minutes(stale_after_minutes);
    let mut recovered = Vec::new();

    for item in items {
        let Some(agent_id) = item.claimed_by.clone() else {
            continue;
        };
        let stale = match item.claim_expires_at.as_deref() {
            Some(expires_at) => timestamp_is_before_or_equal(expires_at, OffsetDateTime::now_utc()),
            None => item
                .claimed_at
                .as_deref()
                .map(|claimed_at| timestamp_is_before_or_equal(claimed_at, cutoff))
                .unwrap_or(false),
        };
        if !stale {
            continue;
        }

        let claim = RecoveredClaimView {
            item_id: item.id,
            agent_id: agent_id.clone(),
            claimed_at: item.claimed_at.clone(),
        };
        claim_returns::release_item(
            store,
            project_name,
            item.id,
            &agent_id,
            Some(format!(
                "Recovered stale claim after {stale_after_minutes} minute(s)."
            )),
            ReleaseAutomationDisposition::Claimable,
        )
        .await?;
        recovered.push(claim);
    }

    Ok(recovered)
}

fn timestamp_is_before_or_equal(value: &str, cutoff: OffsetDateTime) -> bool {
    OffsetDateTime::parse(value, &Rfc3339)
        .map(|timestamp| timestamp <= cutoff)
        .unwrap_or(false)
}
