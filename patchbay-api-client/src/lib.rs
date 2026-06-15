use patchbay_types::{
    AddCommentRequest, AgentRunView, ApiError, ClaimWorkItemRequest, ClaimWorkItemResponse,
    CommentView, CreateWorkItemLabelRequest, CreateWorkItemRequest, DeleteWorkItemLabelResponse,
    FinishWorkItemRequest, ProgressWorkItemRequest, ProjectLabelView, ProjectMemoryCompactionView,
    ProjectMemoryEventView, ProjectMemoryUpdateView, ProjectMemoryView, ProjectSettingsView,
    ProjectView, ReleaseWorkItemRequest, RunLogView, UpdateProjectMemoryRequest,
    UpdateWorkItemLabelRequest, UpdateWorkItemRequest, WorkItemLabelView, WorkItemView,
};
use rootcause::{Result, prelude::*};
use serde::{Serialize, de::DeserializeOwned};

#[derive(Clone, Debug)]
pub struct PatchbayClient {
    base_url: String,
    http: reqwest::Client,
}

impl PatchbayClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            http: reqwest::Client::new(),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub async fn get_project(&self, project: &str) -> Result<ProjectView> {
        self.get(&project_path(project, "")).await
    }

    pub async fn get_project_settings(&self, project: &str) -> Result<ProjectSettingsView> {
        self.get(&project_path(project, "/settings")).await
    }

    pub async fn get_project_memory(&self, project: &str) -> Result<ProjectMemoryView> {
        self.get(&project_path(project, "/memory")).await
    }

    pub async fn list_project_memory_events(
        &self,
        project: &str,
    ) -> Result<Vec<ProjectMemoryEventView>> {
        self.get(&project_path(project, "/memory/events")).await
    }

    pub async fn set_project_memory(
        &self,
        project: &str,
        request: &UpdateProjectMemoryRequest,
    ) -> Result<ProjectMemoryUpdateView> {
        self.put(&project_path(project, "/memory"), request).await
    }

    pub async fn append_project_memory(
        &self,
        project: &str,
        request: &UpdateProjectMemoryRequest,
    ) -> Result<ProjectMemoryUpdateView> {
        self.post(&project_path(project, "/memory/append"), request)
            .await
    }

    pub async fn compact_project_memory_events(
        &self,
        project: &str,
    ) -> Result<ProjectMemoryCompactionView> {
        self.post(&project_path(project, "/memory/events/compact"), &())
            .await
    }

    pub async fn list_items(
        &self,
        project: &str,
        state: Option<&str>,
    ) -> Result<Vec<WorkItemView>> {
        let mut path = project_path(project, "/items");
        if let Some(state) = state {
            path.push_str("?state=");
            path.push_str(&urlencoding::encode(state));
        }
        self.get(&path).await
    }

    pub async fn list_project_labels(&self, project: &str) -> Result<Vec<ProjectLabelView>> {
        self.get(&project_path(project, "/labels")).await
    }

    pub async fn create_item(
        &self,
        project: &str,
        request: &CreateWorkItemRequest,
    ) -> Result<WorkItemView> {
        self.post(&project_path(project, "/items"), request).await
    }

    pub async fn get_item(&self, project: &str, item_id: i64) -> Result<WorkItemView> {
        self.get(&project_path(project, &format!("/items/{item_id}")))
            .await
    }

    pub async fn update_item(
        &self,
        project: &str,
        item_id: i64,
        request: &UpdateWorkItemRequest,
    ) -> Result<WorkItemView> {
        self.patch(
            &project_path(project, &format!("/items/{item_id}")),
            request,
        )
        .await
    }

    pub async fn list_item_labels(
        &self,
        project: &str,
        item_id: i64,
    ) -> Result<Vec<WorkItemLabelView>> {
        self.get(&project_path(project, &format!("/items/{item_id}/labels")))
            .await
    }

    pub async fn add_item_label(
        &self,
        project: &str,
        item_id: i64,
        request: &CreateWorkItemLabelRequest,
        expect_version: Option<i64>,
    ) -> Result<WorkItemView> {
        let mut path = project_path(project, &format!("/items/{item_id}/labels"));
        if let Some(expect_version) = expect_version {
            path.push_str("?expect_version=");
            path.push_str(&expect_version.to_string());
        }
        self.post(&path, request).await
    }

    pub async fn update_item_label(
        &self,
        project: &str,
        item_id: i64,
        label_id: i64,
        request: &UpdateWorkItemLabelRequest,
    ) -> Result<WorkItemView> {
        self.patch(
            &project_path(project, &format!("/items/{item_id}/labels/{label_id}")),
            request,
        )
        .await
    }

    pub async fn delete_item_label(
        &self,
        project: &str,
        item_id: i64,
        label_id: i64,
        expect_version: Option<i64>,
    ) -> Result<DeleteWorkItemLabelResponse> {
        let mut path = project_path(project, &format!("/items/{item_id}/labels/{label_id}"));
        if let Some(expect_version) = expect_version {
            path.push_str("?expect_version=");
            path.push_str(&expect_version.to_string());
        }
        self.delete(&path).await
    }

    pub async fn claim_item(
        &self,
        project: &str,
        request: &ClaimWorkItemRequest,
    ) -> Result<ClaimWorkItemResponse> {
        self.post(&project_path(project, "/items/claim"), request)
            .await
    }

    pub async fn progress_item(
        &self,
        project: &str,
        item_id: i64,
        request: &ProgressWorkItemRequest,
    ) -> Result<CommentView> {
        self.post(
            &project_path(project, &format!("/items/{item_id}/progress")),
            request,
        )
        .await
    }

    pub async fn finish_item(
        &self,
        project: &str,
        item_id: i64,
        request: &FinishWorkItemRequest,
    ) -> Result<WorkItemView> {
        self.post(
            &project_path(project, &format!("/items/{item_id}/finish")),
            request,
        )
        .await
    }

    pub async fn release_item(
        &self,
        project: &str,
        item_id: i64,
        request: &ReleaseWorkItemRequest,
    ) -> Result<WorkItemView> {
        self.post(
            &project_path(project, &format!("/items/{item_id}/release")),
            request,
        )
        .await
    }

    pub async fn list_comments(&self, project: &str, item_id: i64) -> Result<Vec<CommentView>> {
        self.get(&project_path(
            project,
            &format!("/items/{item_id}/comments"),
        ))
        .await
    }

    pub async fn add_comment(
        &self,
        project: &str,
        item_id: i64,
        request: &AddCommentRequest,
    ) -> Result<CommentView> {
        self.post(
            &project_path(project, &format!("/items/{item_id}/comments")),
            request,
        )
        .await
    }

    pub async fn list_runs(&self, project: &str, limit: Option<u64>) -> Result<Vec<AgentRunView>> {
        let mut path = project_path(project, "/automation/runs");
        if let Some(limit) = limit {
            path.push_str("?limit=");
            path.push_str(&limit.to_string());
        }
        self.get(&path).await
    }

    pub async fn read_run_log(&self, project: &str, run_id: i64) -> Result<RunLogView> {
        self.get(&project_path(
            project,
            &format!("/automation/runs/{run_id}/log"),
        ))
        .await
    }

    async fn get<T>(&self, path: &str) -> Result<T>
    where
        T: DeserializeOwned,
    {
        self.send(self.http.get(self.url(path))).await
    }

    async fn post<T, B>(&self, path: &str, body: &B) -> Result<T>
    where
        T: DeserializeOwned,
        B: Serialize + ?Sized,
    {
        self.send(self.http.post(self.url(path)).json(body)).await
    }

    async fn put<T, B>(&self, path: &str, body: &B) -> Result<T>
    where
        T: DeserializeOwned,
        B: Serialize + ?Sized,
    {
        self.send(self.http.put(self.url(path)).json(body)).await
    }

    async fn patch<T, B>(&self, path: &str, body: &B) -> Result<T>
    where
        T: DeserializeOwned,
        B: Serialize + ?Sized,
    {
        self.send(self.http.patch(self.url(path)).json(body)).await
    }

    async fn delete<T>(&self, path: &str) -> Result<T>
    where
        T: DeserializeOwned,
    {
        self.send(self.http.delete(self.url(path))).await
    }

    async fn send<T>(&self, request: reqwest::RequestBuilder) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let response = request
            .send()
            .await
            .context_with(|| format!("failed to call Patchbay API at {}", self.base_url))?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .context("failed to read Patchbay API response")?;

        if !status.is_success() {
            if let Ok(error) = serde_json::from_slice::<ApiError>(&bytes) {
                bail!("{}", error.error);
            }
            let body = String::from_utf8_lossy(&bytes);
            bail!("Patchbay API returned {status}: {body}");
        }

        Ok(serde_json::from_slice(&bytes).context("failed to decode Patchbay API response")?)
    }

    fn url(&self, path: &str) -> String {
        format!("{}/{}", self.base_url, path.trim_start_matches('/'))
    }
}

fn project_path(project: &str, suffix: &str) -> String {
    format!("/api/projects/{}{}", encode_path_segment(project), suffix)
}

fn encode_path_segment(value: &str) -> String {
    urlencoding::encode(value).into_owned()
}
