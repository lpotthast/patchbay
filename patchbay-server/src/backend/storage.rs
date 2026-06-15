use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use rootcause::{Result, prelude::*};
use sea_orm::{ConnectionTrait, Database, DatabaseConnection, DbBackend, Statement};
use sea_orm_migration::MigratorTrait;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::backend::migrations::Migrator;

#[derive(Clone)]
pub struct Store {
    db: Arc<DatabaseConnection>,
    path: Arc<PathBuf>,
}

impl Store {
    pub async fn open(path: PathBuf) -> Result<Self> {
        let path = absolute_path(path)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).context_with(|| {
                format!("failed to create database directory {}", parent.display())
            })?;
        }

        let url = sqlite_url(&path);
        let db = Database::connect(&url)
            .await
            .context_with(|| format!("failed to open database {}", path.display()))?;
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "PRAGMA foreign_keys = ON".to_owned(),
        ))
        .await
        .context("failed to enable SQLite foreign keys")?;

        Migrator::up(&db, None)
            .await
            .context("failed to apply database migrations")?;

        Ok(Self {
            db: Arc::new(db),
            path: Arc::new(path),
        })
    }

    pub fn db(&self) -> Arc<DatabaseConnection> {
        self.db.clone()
    }

    pub fn path(&self) -> &Path {
        self.path.as_ref().as_path()
    }
}

pub fn default_database_path() -> PathBuf {
    patchbay_home_dir().join("patchbay.sqlite3")
}

pub fn patchbay_home_dir() -> PathBuf {
    if let Some(home) = env::var_os("HOME") {
        return PathBuf::from(home).join(".patchbay");
    }

    PathBuf::from(".patchbay")
}

pub fn utc_now() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

fn sqlite_url(path: &Path) -> String {
    format!("sqlite://{}?mode=rwc", path.display())
}

fn absolute_path(path: PathBuf) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path);
    }
    Ok(env::current_dir()
        .context("failed to read current directory for database path")?
        .join(path))
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Statement};
    use tempfile::TempDir;

    use super::*;

    #[tokio::test]
    async fn migrations_apply_cleanly() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("patchbay.sqlite3");

        Store::open(path.clone()).await.unwrap();

        let second = Store::open(path).await.unwrap();
        let row = second
            .db()
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS count FROM seaql_migrations".to_owned(),
            ))
            .await
            .unwrap()
            .unwrap();
        let count: i64 = row.try_get("", "count").unwrap();

        assert_eq!(count as usize, Migrator::migrations().len());
    }

    #[test]
    fn migration_history_names_all_current_migrations() {
        let migrations = Migrator::migrations();
        let names = migrations
            .iter()
            .map(|migration| migration.name())
            .collect::<Vec<_>>();
        let expected = [
            "migrations",
            "m20260612_000002_add_phase_two_coordination",
            "m20260612_000003_add_project_context",
            "m20260612_000004_add_phase_three_automation",
            "m20260612_000005_add_phase_three_workspace_policy",
            "m20260612_000006_add_phase_four_hardening",
            "m20260612_000007_add_project_default_agent_tool",
            "m20260613_000008_move_run_settings_into_projects",
            "m20260613_000009_drop_claude_code_support",
            "m20260613_000010_rename_project_repo_path",
            "m20260613_000011_add_project_path_status",
            "m20260613_000012_add_automation_run_configuration",
            "m20260614_000013_remove_automation_trigger_dry_run",
            "m20260614_000014_add_automation_run_trigger_origin",
            "m20260614_000015_add_project_memory_events",
            "m20260615_000016_remove_work_item_automation_claimable",
            "m20260615_000017_add_labels_and_swim_lanes",
            "m20260615_000018_add_automation_work_item_selectors",
            "m20260615_000018_rename_automation_activation_require_schedule",
            "m20260615_000019_add_automation_work_item_selectors",
            "m20260615_000020_rename_automation_activation_require_schedule",
            "m20260615_000021_add_work_item_state_label_read_view",
        ];

        assert_eq!(names.as_slice(), expected.as_slice());
    }
}
