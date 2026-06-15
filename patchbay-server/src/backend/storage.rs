use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result};
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
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create database directory {}", parent.display())
            })?;
        }

        let url = sqlite_url(&path);
        let db = Database::connect(&url)
            .await
            .with_context(|| format!("failed to open database {}", path.display()))?;
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
}
