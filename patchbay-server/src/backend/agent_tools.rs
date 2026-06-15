use std::{
    env, fs,
    path::{Path, PathBuf},
    str::FromStr,
};

use anyhow::{Context, Result, bail};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
};

use crate::{
    backend::{
        entities::agent_tool::{self, AgentTool, AgentToolActiveModel, AgentToolModel},
        storage::{Store, utc_now},
    },
    shared::view_models::{AgentToolName, AgentToolView},
};

pub async fn discover_tools(store: &Store) -> Result<Vec<AgentToolView>> {
    let mut tools = Vec::new();
    for tool in AgentToolName::all() {
        tools.push(discover_tool(store, tool).await?);
    }
    Ok(tools)
}

pub async fn list_tools(store: &Store) -> Result<Vec<AgentToolView>> {
    let tools = AgentTool::find()
        .filter(agent_tool::Column::ToolName.eq(AgentToolName::Codex.as_storage()))
        .order_by_asc(agent_tool::Column::ToolName)
        .all(store.db().as_ref())
        .await
        .context("failed to list agent tools")?;

    tools.into_iter().map(model_to_view).collect()
}

pub async fn set_tool_path(
    store: &Store,
    tool: AgentToolName,
    path: PathBuf,
) -> Result<AgentToolView> {
    if path.as_os_str().is_empty() {
        bail!("agent tool path cannot be empty");
    }

    let existing = find_tool_model(store, tool).await?;
    let now = utc_now();
    let model = if let Some(existing) = existing {
        let mut active: AgentToolActiveModel = existing.into();
        active.executable_path = Set(Some(path.to_string_lossy().into_owned()));
        active.updated_at = Set(now);
        active
            .update(store.db().as_ref())
            .await
            .context("failed to update agent tool path")?
    } else {
        AgentToolActiveModel {
            tool_name: Set(tool.as_storage().to_owned()),
            executable_path: Set(Some(path.to_string_lossy().into_owned())),
            discovered_path: Set(None),
            last_discovered_at: Set(None),
            created_at: Set(now.clone()),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(store.db().as_ref())
        .await
        .context("failed to store agent tool path")?
    };

    model_to_view(model)
}

pub async fn resolve_tool_path(store: &Store, tool: AgentToolName) -> Result<PathBuf> {
    let view = match find_tool_model(store, tool).await? {
        Some(model) => model_to_view(model)?,
        None => discover_tool(store, tool).await?,
    };

    view.effective_path
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("agent tool '{tool}' is not configured or discoverable"))
}

async fn discover_tool(store: &Store, tool: AgentToolName) -> Result<AgentToolView> {
    let discovered_path = find_executable(tool.as_storage());
    let existing = find_tool_model(store, tool).await?;
    let now = utc_now();

    let model = if let Some(existing) = existing {
        let mut active: AgentToolActiveModel = existing.into();
        active.discovered_path =
            Set(discovered_path.map(|path| path.to_string_lossy().into_owned()));
        active.last_discovered_at = Set(Some(now.clone()));
        active.updated_at = Set(now);
        active
            .update(store.db().as_ref())
            .await
            .context("failed to update discovered agent tool path")?
    } else {
        AgentToolActiveModel {
            tool_name: Set(tool.as_storage().to_owned()),
            executable_path: Set(None),
            discovered_path: Set(discovered_path.map(|path| path.to_string_lossy().into_owned())),
            last_discovered_at: Set(Some(now.clone())),
            created_at: Set(now.clone()),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(store.db().as_ref())
        .await
        .context("failed to store discovered agent tool path")?
    };

    model_to_view(model)
}

async fn find_tool_model(store: &Store, tool: AgentToolName) -> Result<Option<AgentToolModel>> {
    AgentTool::find()
        .filter(agent_tool::Column::ToolName.eq(tool.as_storage()))
        .one(store.db().as_ref())
        .await
        .with_context(|| format!("failed to load agent tool '{tool}'"))
}

fn find_executable(name: &str) -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;
    find_executable_in_path_var(name, &path_var)
}

fn find_executable_in_path_var(name: &str, path_var: &std::ffi::OsStr) -> Option<PathBuf> {
    env::split_paths(path_var).find_map(|directory| {
        let candidate = directory.join(name);
        if is_executable_file(&candidate) {
            Some(candidate)
        } else {
            None
        }
    })
}

fn is_executable_file(path: &Path) -> bool {
    fs::metadata(path)
        .map(|metadata| metadata.is_file())
        .unwrap_or(false)
}

fn model_to_view(tool: AgentToolModel) -> Result<AgentToolView> {
    let tool_name = AgentToolName::from_str(&tool.tool_name)?;
    let effective_path = tool
        .executable_path
        .clone()
        .or_else(|| tool.discovered_path.clone());
    Ok(AgentToolView {
        id: tool.id,
        tool_name,
        executable_path: tool.executable_path,
        discovered_path: tool.discovered_path,
        effective_path,
        last_discovered_at: tool.last_discovered_at,
        created_at: tool.created_at,
        updated_at: tool.updated_at,
    })
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    async fn test_store() -> (TempDir, Store) {
        let temp = TempDir::new().unwrap();
        let store = Store::open(temp.path().join("patchbay.sqlite3"))
            .await
            .unwrap();
        (temp, store)
    }

    #[test]
    fn executable_lookup_uses_path_order() {
        let temp = TempDir::new().unwrap();
        let bin = temp.path().join("bin");
        fs::create_dir(&bin).unwrap();
        let tool = bin.join("codex");
        fs::write(&tool, "#!/bin/sh\n").unwrap();

        let found = find_executable_in_path_var("codex", bin.as_os_str()).unwrap();

        assert_eq!(found, tool);
    }

    #[tokio::test]
    async fn explicit_tool_path_becomes_effective_path() {
        let (_temp, store) = test_store().await;

        let tool = set_tool_path(&store, AgentToolName::Codex, PathBuf::from("/bin/echo"))
            .await
            .unwrap();

        assert_eq!(tool.executable_path.as_deref(), Some("/bin/echo"));
        assert_eq!(tool.effective_path.as_deref(), Some("/bin/echo"));
    }
}
