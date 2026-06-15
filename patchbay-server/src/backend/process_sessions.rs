use std::{collections::HashMap, sync::Arc};

use tokio::sync::{Mutex, watch};

use crate::{
    backend::storage::utc_now,
    shared::view_models::{AgentRunOutputPiece, ProcessSessionView},
};

#[cfg(test)]
use crate::shared::view_models::AgentRunOutputKind;

const MAX_SESSION_OUTPUT_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug)]
pub struct ProcessSessionRegistry {
    sessions: Arc<Mutex<HashMap<i64, ProcessSession>>>,
}

impl ProcessSessionRegistry {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn begin(&self, start: ProcessSessionStart) -> watch::Receiver<bool> {
        let now = utc_now();
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let session = ProcessSession {
            run_id: start.run_id,
            project_name: start.project_name,
            tool_name: start.tool_name,
            command: start.command,
            working_dir: start.working_dir,
            process_id: None,
            output: Vec::new(),
            cancel_tx,
            started_at: now.clone(),
            updated_at: now,
        };
        self.sessions.lock().await.insert(session.run_id, session);
        cancel_rx
    }

    pub async fn append_output_piece(&self, run_id: i64, piece: AgentRunOutputPiece) {
        if let Some(session) = self.sessions.lock().await.get_mut(&run_id) {
            session.output.push(piece);
            trim_output_pieces(&mut session.output, MAX_SESSION_OUTPUT_BYTES);
            session.updated_at = utc_now();
        }
    }

    pub async fn finish(&self, run_id: i64) {
        self.sessions.lock().await.remove(&run_id);
    }

    pub async fn list_for_project(&self, project_name: &str) -> Vec<ProcessSessionView> {
        let mut sessions = self
            .sessions
            .lock()
            .await
            .values()
            .filter(|session| session.project_name == project_name)
            .map(ProcessSessionView::from)
            .collect::<Vec<_>>();
        sessions.sort_by_key(|session| session.run_id);
        sessions
    }

    pub async fn list_all(&self) -> Vec<ProcessSessionView> {
        let mut sessions = self
            .sessions
            .lock()
            .await
            .values()
            .map(ProcessSessionView::from)
            .collect::<Vec<_>>();
        sessions.sort_by_key(|session| session.run_id);
        sessions
    }

    pub async fn cancel_project(&self, project_name: &str) -> usize {
        let senders = self
            .sessions
            .lock()
            .await
            .values()
            .filter(|session| session.project_name == project_name)
            .map(|session| session.cancel_tx.clone())
            .collect::<Vec<_>>();
        for sender in &senders {
            let _ = sender.send(true);
        }
        senders.len()
    }

    pub async fn cancel_all(&self) -> usize {
        let senders = self
            .sessions
            .lock()
            .await
            .values()
            .map(|session| session.cancel_tx.clone())
            .collect::<Vec<_>>();
        for sender in &senders {
            let _ = sender.send(true);
        }
        senders.len()
    }
}

impl Default for ProcessSessionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug)]
pub struct ProcessSessionStart {
    pub run_id: i64,
    pub project_name: String,
    pub tool_name: String,
    pub command: String,
    pub working_dir: String,
}

#[derive(Clone, Debug)]
struct ProcessSession {
    run_id: i64,
    project_name: String,
    tool_name: String,
    command: String,
    working_dir: String,
    process_id: Option<i64>,
    output: Vec<AgentRunOutputPiece>,
    cancel_tx: watch::Sender<bool>,
    started_at: String,
    updated_at: String,
}

impl From<&ProcessSession> for ProcessSessionView {
    fn from(session: &ProcessSession) -> Self {
        Self {
            run_id: session.run_id,
            project_name: session.project_name.clone(),
            tool_name: session.tool_name.clone(),
            command: session.command.clone(),
            working_dir: session.working_dir.clone(),
            process_id: session.process_id,
            output: session.output.clone(),
            started_at: session.started_at.clone(),
            updated_at: session.updated_at.clone(),
        }
    }
}

fn trim_output_pieces(pieces: &mut Vec<AgentRunOutputPiece>, max_bytes: usize) {
    while pieces.len() > 1 && output_pieces_size(pieces) > max_bytes {
        pieces.remove(0);
    }
}

fn output_pieces_size(pieces: &[AgentRunOutputPiece]) -> usize {
    pieces.iter().map(output_piece_size).sum()
}

fn output_piece_size(piece: &AgentRunOutputPiece) -> usize {
    piece.timestamp.len()
        + piece.source.len()
        + piece.item_id.as_deref().map(str::len).unwrap_or_default()
        + piece.title.len()
        + piece.body.len()
        + piece.metadata.to_string().len()
}

#[cfg(test)]
fn test_piece(sequence: u64, body: &str) -> AgentRunOutputPiece {
    AgentRunOutputPiece {
        sequence,
        timestamp: utc_now(),
        kind: AgentRunOutputKind::ModelMessage,
        source: "test".to_owned(),
        item_id: None,
        title: "stdout".to_owned(),
        body: body.to_owned(),
        metadata: serde_json::json!({ "stream": "stdout" }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn session_output_is_retained_in_memory() {
        let sessions = ProcessSessionRegistry::new();
        sessions
            .begin(ProcessSessionStart {
                run_id: 7,
                project_name: "demo".to_owned(),
                tool_name: "codex".to_owned(),
                command: "codex app-server turn prompt.md".to_owned(),
                working_dir: "/tmp/demo".to_owned(),
            })
            .await;

        sessions
            .append_output_piece(7, test_piece(1, "line one"))
            .await;

        let active = sessions.list_for_project("demo").await;
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].output.len(), 1);
        assert_eq!(active[0].output[0].kind, AgentRunOutputKind::ModelMessage);
        assert_eq!(active[0].output[0].body, "line one");
    }
}
