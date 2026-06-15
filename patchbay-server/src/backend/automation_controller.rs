use std::{collections::HashMap, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use tokio::{
    sync::{Mutex, watch},
    task::JoinHandle,
};

use crate::{
    backend::{
        automation::{self, StartAutomation},
        items,
        process_sessions::ProcessSessionRegistry,
        projects,
        storage::Store,
    },
    shared::view_models::{AgentRunStatus, AutomationMode, WorkState},
};

const IDLE_POLL_INTERVAL: Duration = Duration::from_secs(5);
const FAILURE_BACKOFF: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, Default)]
pub struct AutomationController {
    projects: Arc<Mutex<HashMap<String, ProjectAutomation>>>,
}

#[derive(Debug)]
struct ProjectAutomation {
    shutdown: watch::Sender<bool>,
    handle: JoinHandle<()>,
}

impl AutomationController {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn start_project(
        &self,
        store: Store,
        project_name: String,
        sessions: ProcessSessionRegistry,
    ) -> Result<()> {
        projects::get_project(&store, &project_name).await?;

        let mut projects = self.projects.lock().await;
        if projects.contains_key(&project_name) {
            return Ok(());
        }

        let (shutdown, shutdown_rx) = watch::channel(false);
        let handle = tokio::spawn(run_project_automation(
            store,
            project_name.clone(),
            sessions,
            shutdown_rx,
        ));
        projects.insert(project_name, ProjectAutomation { shutdown, handle });
        Ok(())
    }

    pub async fn stop_project(
        &self,
        project_name: &str,
        sessions: &ProcessSessionRegistry,
    ) -> Result<()> {
        let automation = self.projects.lock().await.remove(project_name);
        sessions.cancel_project(project_name).await;
        if let Some(automation) = automation {
            let _ = automation.shutdown.send(true);
            automation
                .handle
                .await
                .context("project automation task failed")?;
        }
        Ok(())
    }

    pub async fn shutdown_all(&self, sessions: &ProcessSessionRegistry) {
        let projects = std::mem::take(&mut *self.projects.lock().await);
        for automation in projects.values() {
            let _ = automation.shutdown.send(true);
        }
        sessions.cancel_all().await;
        for (project_name, automation) in projects {
            if let Err(err) = automation.handle.await {
                eprintln!("project automation task failed for {project_name}: {err:#}");
            }
        }
    }

    pub async fn is_project_running(&self, project_name: &str) -> bool {
        self.projects.lock().await.contains_key(project_name)
    }

    pub async fn active_project_names(&self) -> Vec<String> {
        let mut names = self
            .projects
            .lock()
            .await
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    pub async fn project_cancellations(&self) -> HashMap<String, watch::Receiver<bool>> {
        self.projects
            .lock()
            .await
            .iter()
            .map(|(project_name, automation)| {
                (project_name.clone(), automation.shutdown.subscribe())
            })
            .collect()
    }
}

async fn run_project_automation(
    store: Store,
    project_name: String,
    sessions: ProcessSessionRegistry,
    mut shutdown: watch::Receiver<bool>,
) {
    loop {
        if *shutdown.borrow() {
            break;
        }

        match items::has_claimable_item(&store, &project_name, WorkState::Open).await {
            Ok(true) => {}
            Ok(false) => {
                if wait_or_shutdown(IDLE_POLL_INTERVAL, &mut shutdown).await {
                    break;
                }
                continue;
            }
            Err(err) => {
                eprintln!("project automation poll failed for {project_name}: {err:#}");
                if wait_or_shutdown(FAILURE_BACKOFF, &mut shutdown).await {
                    break;
                }
                continue;
            }
        }

        let result = automation::start_automation_with_sessions_until(
            &store,
            &project_name,
            StartAutomation {
                mode: AutomationMode::Execute,
                tool: None,
                work_item_id: None,
                extra_prompt: None,
                trigger: None,
            },
            Some(sessions.clone()),
            Some(shutdown.clone()),
        )
        .await;

        match result {
            Ok(run) if run.status == AgentRunStatus::Failed => {
                if wait_or_shutdown(FAILURE_BACKOFF, &mut shutdown).await {
                    break;
                }
            }
            Ok(run) if run.status == AgentRunStatus::Cancelled => break,
            Ok(_) => {}
            Err(err) => {
                if *shutdown.borrow() {
                    break;
                }
                eprintln!("project automation run failed for {project_name}: {err:#}");
                if wait_or_shutdown(FAILURE_BACKOFF, &mut shutdown).await {
                    break;
                }
            }
        }
    }
}

async fn wait_or_shutdown(duration: Duration, shutdown: &mut watch::Receiver<bool>) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(duration) => *shutdown.borrow(),
        changed = shutdown.changed() => changed.is_err() || *shutdown.borrow(),
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::backend::projects::{CreateProject, create_project};

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
                system_prompt: None,
                memory: None,
            },
        )
        .await
        .unwrap();
        (temp, store)
    }

    #[tokio::test]
    async fn controller_idles_without_creating_runs_when_no_work_exists() {
        let (_temp, store) = test_store().await;
        let controller = AutomationController::new();
        let sessions = ProcessSessionRegistry::new();

        controller
            .start_project(store.clone(), "demo".to_owned(), sessions.clone())
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(25)).await;

        assert!(controller.is_project_running("demo").await);
        assert!(
            automation::list_runs(&store, "demo", None)
                .await
                .unwrap()
                .is_empty()
        );

        controller.stop_project("demo", &sessions).await.unwrap();

        assert!(!controller.is_project_running("demo").await);
    }
}
