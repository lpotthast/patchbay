use std::sync::{Arc, LazyLock, RwLock};

use crate::{
    backend::{
        automation_controller::AutomationController, process_sessions::ProcessSessionRegistry,
        storage::Store,
    },
    shared::view_models::CodexAppServerStatusView,
};

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) store: Store,
    pub(crate) sessions: ProcessSessionRegistry,
    pub(crate) automation_controller: AutomationController,
    pub(crate) codex_status: Arc<tokio::sync::RwLock<CodexAppServerStatusView>>,
}

static APP_STATE: LazyLock<RwLock<Option<AppState>>> = LazyLock::new(|| RwLock::new(None));

pub(crate) fn app_state() -> AppState {
    APP_STATE
        .read()
        .expect("Patchbay app state lock is poisoned")
        .clone()
        .expect("Patchbay app state must be installed before rendering")
}

pub(crate) fn install_app_state(state: AppState) {
    *APP_STATE
        .write()
        .expect("Patchbay app state lock is poisoned") = Some(state);
}
