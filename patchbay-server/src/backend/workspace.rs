use std::{
    env,
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    process::Stdio,
};

use anyhow::{Context, Result, bail};

use crate::backend::{projects, storage::Store};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WorkspaceOpenTarget {
    Folder,
    Ide,
}

impl WorkspaceOpenTarget {
    pub(crate) fn parse(value: &str) -> Result<Self> {
        match value.trim() {
            "folder" => Ok(Self::Folder),
            "ide" => Ok(Self::Ide),
            other => bail!("workspace open target must be folder or ide, got '{other}'"),
        }
    }
}

#[derive(Clone, Debug)]
struct WorkspaceOpenConfig {
    os: String,
    ide_name: String,
}

impl WorkspaceOpenConfig {
    fn from_env() -> Self {
        Self {
            os: env::consts::OS.to_owned(),
            ide_name: env::var("PATCHBAY_WORKSPACE_IDE")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(default_ide_name),
        }
    }

    fn command_for(
        &self,
        target: WorkspaceOpenTarget,
        path: &Path,
    ) -> Result<WorkspaceOpenCommand> {
        match target {
            WorkspaceOpenTarget::Folder => folder_open_command(&self.os, path),
            WorkspaceOpenTarget::Ide => ide_open_command(&self.os, &self.ide_name, path),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct WorkspaceOpenCommand {
    program: OsString,
    args: Vec<OsString>,
}

impl WorkspaceOpenCommand {
    async fn run(self) -> Result<()> {
        let output = tokio::process::Command::new(&self.program)
            .args(&self.args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .await
            .with_context(|| format!("failed to start {}", self.program.to_string_lossy()))?;

        if output.status.success() {
            return Ok(());
        }

        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "{} failed{}",
            self.program.to_string_lossy(),
            if stderr.trim().is_empty() {
                String::new()
            } else {
                format!(": {}", stderr.trim())
            }
        )
    }
}

pub(crate) async fn project_workspace_path(store: &Store, project_name: &str) -> Result<PathBuf> {
    let project = projects::get_project(store, project_name).await?;
    let path = project
        .path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .ok_or_else(|| anyhow::anyhow!("project '{project_name}' has no workspace path"))?;
    existing_workspace_path(path)
}

pub(crate) fn existing_workspace_path(path: impl AsRef<Path>) -> Result<PathBuf> {
    let path = path.as_ref();
    let canonical = path
        .canonicalize()
        .with_context(|| format!("workspace path '{}' does not exist", path.display()))?;
    if !canonical.is_dir() {
        bail!(
            "workspace path '{}' is not a directory",
            canonical.display()
        );
    }
    Ok(canonical)
}

pub(crate) async fn open_workspace_path(
    target: WorkspaceOpenTarget,
    path: impl AsRef<Path>,
) -> Result<()> {
    let path = existing_workspace_path(path)?;
    let command = WorkspaceOpenConfig::from_env().command_for(target, &path)?;
    command.run().await
}

fn folder_open_command(os: &str, path: &Path) -> Result<WorkspaceOpenCommand> {
    match os {
        "macos" => Ok(command("open", [path.as_os_str()])),
        "linux" => Ok(command("xdg-open", [path.as_os_str()])),
        "windows" => Ok(command("explorer", [path.as_os_str()])),
        other => bail!("folder opening is not supported on {other}"),
    }
}

fn ide_open_command(os: &str, ide_name: &str, path: &Path) -> Result<WorkspaceOpenCommand> {
    let ide_name = ide_name.trim();
    if ide_name.is_empty() {
        bail!("PATCHBAY_WORKSPACE_IDE cannot be empty");
    }

    match os {
        "macos" => Ok(command(
            "open",
            [OsStr::new("-a"), OsStr::new(ide_name), path.as_os_str()],
        )),
        _ => Ok(command(ide_name, [path.as_os_str()])),
    }
}

fn command<const N: usize>(program: impl AsRef<OsStr>, args: [&OsStr; N]) -> WorkspaceOpenCommand {
    WorkspaceOpenCommand {
        program: program.as_ref().to_owned(),
        args: args.into_iter().map(OsStr::to_owned).collect(),
    }
}

fn default_ide_name() -> String {
    match env::consts::OS {
        "macos" => "RustRover".to_owned(),
        "windows" => "rustrover64.exe".to_owned(),
        _ => "rustrover".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_ide_command_uses_open_application() {
        let path = Path::new("/tmp/demo");
        let config = WorkspaceOpenConfig {
            os: "macos".to_owned(),
            ide_name: "RustRover".to_owned(),
        };

        let command = config.command_for(WorkspaceOpenTarget::Ide, path).unwrap();

        assert_eq!(command.program, OsString::from("open"));
        assert_eq!(
            command.args,
            vec![
                OsString::from("-a"),
                OsString::from("RustRover"),
                OsString::from("/tmp/demo"),
            ]
        );
    }

    #[test]
    fn linux_folder_command_uses_xdg_open() {
        let command = folder_open_command("linux", Path::new("/tmp/demo")).unwrap();

        assert_eq!(command.program, OsString::from("xdg-open"));
        assert_eq!(command.args, vec![OsString::from("/tmp/demo")]);
    }

    #[test]
    fn missing_workspace_path_is_rejected() {
        let err = existing_workspace_path("/definitely/not/a/patchbay/workspace")
            .unwrap_err()
            .to_string();

        assert!(err.contains("does not exist"));
    }
}
