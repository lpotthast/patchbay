use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use leptos::prelude::get_configuration;
use rootcause::Result;

use crate::backend::{
    app_state::{AppState, install_app_state},
    automation,
    automation_controller::AutomationController,
    automation_triggers, codex_app_server, crudkit_resources, events, http,
    process_sessions::ProcessSessionRegistry,
    projects,
    storage::Store,
};

const ACTIVE_SESSION_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(30);

pub async fn serve(store: Store, bind: SocketAddr) -> Result<()> {
    automation::set_server_api_url(local_api_url(bind));
    events::install();

    let contexts = crudkit_resources::build_contexts(store.clone());
    let sessions = ProcessSessionRegistry::new();
    let automation_controller = AutomationController::new();
    let codex_status = codex_app_server::app_server_status(&store).await;
    if !codex_status.usable {
        tracing::warn!(
            "{}",
            codex_app_server::operator_guidance(&codex_status).join("\n")
        );
    }
    let codex_status = Arc::new(tokio::sync::RwLock::new(codex_status));
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let state = AppState {
        store: store.clone(),
        sessions: sessions.clone(),
        automation_controller: automation_controller.clone(),
        codex_status: codex_status.clone(),
    };
    install_app_state(state.clone());

    let mut leptos_options = get_configuration(None)?.leptos_options;
    leptos_options.site_addr = bind;

    projects::spawn_path_status_checker_until(store.clone(), shutdown_rx.clone());
    automation_triggers::spawn_scheduler_until(
        store.clone(),
        Some(sessions.clone()),
        automation_controller.clone(),
        shutdown_rx.clone(),
    );
    codex_app_server::spawn_status_refresher_until(
        store.clone(),
        codex_status,
        shutdown_rx.clone(),
    );

    let app = http::router(state, contexts, leptos_options);
    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!(url = %format_args!("http://{bind}"), "Serving Patchbay");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(
            store,
            sessions,
            automation_controller,
            shutdown_tx,
        ))
        .await?;
    Ok(())
}

fn local_api_url(bind: SocketAddr) -> String {
    let host = match bind.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => "127.0.0.1".to_owned(),
        IpAddr::V4(ip) => ip.to_string(),
        IpAddr::V6(ip) if ip.is_unspecified() => "127.0.0.1".to_owned(),
        IpAddr::V6(ip) => format!("[{ip}]"),
    };
    format!("http://{host}:{}", bind.port())
}

async fn shutdown_signal(
    store: Store,
    sessions: ProcessSessionRegistry,
    automation_controller: AutomationController,
    shutdown_tx: tokio::sync::watch::Sender<bool>,
) {
    wait_for_shutdown_signal().await;
    let _ = shutdown_tx.send(true);
    automation_controller.shutdown_all(&sessions).await;
    cancel_active_sessions(&store, &sessions).await;
}

async fn wait_for_shutdown_signal() {
    let ctrl_c = async {
        if let Err(err) = tokio::signal::ctrl_c().await {
            tracing::error!(%err, "failed to install Ctrl+C handler");
        }
    };

    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let terminate = async {
            match signal(SignalKind::terminate()) {
                Ok(mut signal) => {
                    signal.recv().await;
                }
                Err(err) => {
                    tracing::error!(%err, "failed to install SIGTERM handler");
                    std::future::pending::<()>().await;
                }
            }
        };

        tokio::select! {
            _ = ctrl_c => {},
            _ = terminate => {},
        }
    }

    #[cfg(not(unix))]
    {
        ctrl_c.await;
    }
}

async fn cancel_active_sessions(store: &Store, sessions: &ProcessSessionRegistry) {
    let active = sessions.list_all().await;
    let mut projects = active
        .into_iter()
        .map(|session| session.project_name)
        .collect::<Vec<_>>();
    projects.sort();
    projects.dedup();

    sessions.cancel_all().await;
    if let Err(_elapsed) = tokio::time::timeout(ACTIVE_SESSION_SHUTDOWN_TIMEOUT, async {
        loop {
            if sessions.list_all().await.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    {
        tracing::warn!("timed out waiting for active automation sessions to stop");
    }

    for project in projects {
        if let Err(err) = automation::stop_automation(store, &project).await {
            tracing::error!(
                project = %project,
                error = %format_args!("{err:#}"),
                "failed to mark running automation cancelled"
            );
        }
    }
}
