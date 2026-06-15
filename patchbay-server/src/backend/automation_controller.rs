use std::{collections::HashMap, sync::Arc};

use rootcause::{Result, prelude::*};
use tokio::{
    sync::{Mutex, watch},
    task::JoinHandle,
};

use crate::backend::{events, process_sessions::ProcessSessionRegistry, projects, storage::Store};

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
        projects.insert(project_name.clone(), ProjectAutomation { shutdown, handle });
        events::publish_automation_changed(&project_name);
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
        events::publish_automation_changed(project_name);
        Ok(())
    }

    pub async fn shutdown_all(&self, sessions: &ProcessSessionRegistry) {
        let projects = std::mem::take(&mut *self.projects.lock().await);
        let project_names = projects.keys().cloned().collect::<Vec<_>>();
        for automation in projects.values() {
            let _ = automation.shutdown.send(true);
        }
        sessions.cancel_all().await;
        for (project_name, automation) in projects {
            if let Err(err) = automation.handle.await {
                eprintln!("project automation task failed for {project_name}: {err:#}");
            }
        }
        for project_name in project_names {
            events::publish_automation_changed(&project_name);
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
    _store: Store,
    _project_name: String,
    _sessions: ProcessSessionRegistry,
    mut shutdown: watch::Receiver<bool>,
) {
    while !*shutdown.borrow() {
        if shutdown.changed().await.is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tempfile::TempDir;

    use super::*;
    use crate::backend::{
        automation,
        projects::{CreateProject, create_project},
    };

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
                default_agent_reasoning_effort: None,
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
