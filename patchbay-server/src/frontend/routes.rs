use leptos_routes::routes;

#[routes(with_views, fallback = "PageErr404")]
pub mod routes {
    use crate::frontend::{
        MainLayout, PageApiDocs, PageBoard, PageCodex, PageErr404, PageError, PageItem,
        PageProjects, PageRunLog, PageTriggers,
    };

    #[route("/", layout = "MainLayout", fallback = "PageBoard")]
    pub mod root {
        #[route("/projects", view = "PageProjects")]
        pub mod projects {}

        #[route("/triggers", view = "PageTriggers")]
        pub mod triggers {}

        #[route("/codex", view = "PageCodex")]
        pub mod codex {}

        #[route("/api/docs", view = "PageApiDocs")]
        pub mod api_docs {}

        #[route("/error", view = "PageError")]
        pub mod error {}

        #[route("/projects/:project/items/:item_id", view = "PageItem")]
        pub mod item {}

        #[route("/projects/:project/automation/runs/:run_id/log", view = "PageRunLog")]
        pub mod run_log {}
    }
}
