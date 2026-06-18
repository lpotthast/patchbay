use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use codex_app_server_sdk::{
    ClientError, Codex, CodexClient, StdioConfig,
    requests::{ClientInfo, GetAccountParams, InitializeParams},
};
use rootcause::{Result, prelude::*};
use serde_json::Value;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tokio::{sync::watch, time::timeout};

use crate::{
    backend::{
        agent_tools, events,
        storage::{Store, patchbay_home_dir, utc_now},
    },
    shared::view_models::{
        AgentSandboxMode, AgentToolName, CodexAppServerStatusView, CodexAuthSetupView,
        CodexPreconditionView, CodexRateLimitView, CodexUsageSummaryView, ProjectSettingsView,
    },
};

const STATUS_TIMEOUT: Duration = Duration::from_secs(14);
const STATUS_REFRESH_INTERVAL: Duration = Duration::from_secs(30);
const CLIENT_NAME: &str = "patchbay";
const CLIENT_TITLE: &str = "Patchbay";
const CODEX_HOME_DIR: &str = "codex";
const CODEX_CONFIG: &str = r#"# Managed by Patchbay.
# Patchbay provides project memory in each automation prompt and keeps Codex
# memories disabled for deterministic, auditable runs.

[features]
memories = false

[memories]
use_memories = false
generate_memories = false
disable_on_external_context = true
"#;
const PROJECT_RULES_FILE_NAME: &str = "patchbay-git.rules";

pub const CODEX_INSTALL_PROMPT: &str =
    "Install Codex and make sure `codex app-server` is available on PATH.";

pub async fn app_server_status(store: &Store) -> CodexAppServerStatusView {
    let checked_at = utc_now();
    if let Err(err) = ensure_codex_home() {
        return unavailable_status(
            checked_at,
            format!("Codex app-server is unavailable: {err:#}"),
        );
    }
    match agent_tools::resolve_tool_path(store, AgentToolName::Codex)
        .await
        .context_with(|| {
            format!(
                "Patchbay cannot start Codex automation because Codex is not configured or discoverable. {CODEX_INSTALL_PROMPT}"
            )
        }) {
        Ok(path) => app_server_status_for_binary(&path, checked_at).await,
        Err(err) => unavailable_status(checked_at, format!("Codex app-server is unavailable: {err:#}")),
    }
}

pub fn spawn_status_refresher_until(
    store: Store,
    status: Arc<tokio::sync::RwLock<CodexAppServerStatusView>>,
    mut shutdown: watch::Receiver<bool>,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(STATUS_REFRESH_INTERVAL);
        let mut skip_initial_tick = true;
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if skip_initial_tick {
                        skip_initial_tick = false;
                        continue;
                    }
                    let refreshed = app_server_status(&store).await;
                    *status.write().await = refreshed;
                    events::publish_codex_status_changed();
                }
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                }
            }
        }
    });
}

pub async fn logout_current_account(store: &Store) -> Result<CodexAppServerStatusView> {
    ensure_codex_home()?;
    let codex_binary = agent_tools::resolve_tool_path(store, AgentToolName::Codex)
        .await
        .context_with(|| {
            format!(
                "Patchbay cannot log out of Codex because Codex is not configured or discoverable. {CODEX_INSTALL_PROMPT}"
            )
        })?;
    timeout(STATUS_TIMEOUT, async {
        let client = spawn_initialized_client(&codex_binary).await?;
        client
            .account_logout()
            .await
            .context("Codex app-server rejected logout")?;
        Ok::<(), Report>(())
    })
    .await
    .context("timed out while logging out of Codex")??;
    Ok(app_server_status_for_binary(&codex_binary, utc_now()).await)
}

pub async fn ensure_app_server_usable(codex_binary: &Path) -> Result<CodexAppServerStatusView> {
    let status = app_server_status_for_binary(codex_binary, utc_now()).await;
    if status.usable {
        return Ok(status);
    }
    bail!("{}", status.message)
}

async fn app_server_status_for_binary(
    codex_binary: &Path,
    checked_at: String,
) -> CodexAppServerStatusView {
    match timeout(
        STATUS_TIMEOUT,
        inspect_app_server(codex_binary, checked_at.clone()),
    )
    .await
    {
        Ok(status) => status,
        Err(_) => unavailable_status(
            checked_at,
            format!(
                "Codex app-server is unavailable: timed out while checking `{}`.",
                codex_binary.display()
            ),
        ),
    }
}

async fn inspect_app_server(codex_binary: &Path, checked_at: String) -> CodexAppServerStatusView {
    let binary_path = codex_binary.to_string_lossy().into_owned();
    let client = match spawn_initialized_client(codex_binary).await {
        Ok(client) => client,
        Err(err) => {
            return unavailable_status(
                checked_at,
                format!(
                    "Codex app-server is unavailable: failed to initialize `{}`: {err:#}",
                    codex_binary.display()
                ),
            );
        }
    };

    let mut preconditions = vec![CodexPreconditionView {
        name: "Codex app-server".to_owned(),
        ok: true,
        message: format!("Initialized `{}`.", codex_binary.display()),
    }];
    let mut warnings = Vec::new();

    let account_value = match client
        .account_read(GetAccountParams {
            refresh_token: Some(true),
            extra: serde_json::Map::new(),
        })
        .await
        .and_then(|account| {
            serde_json::to_value(account).map_err(codex_app_server_sdk::ClientError::Serialization)
        }) {
        Ok(value) => value,
        Err(err) => {
            let message = auth_failure_message("reading account status", &err)
                .unwrap_or_else(|| format!("Codex account status could not be read: {err}"));
            let auth_setup = codex_auth_setup(codex_binary);
            preconditions.push(CodexPreconditionView {
                name: "Codex account".to_owned(),
                ok: false,
                message: message.clone(),
            });
            return CodexAppServerStatusView {
                available: true,
                usable: false,
                message: format!("Codex SDK is unusable for automation. {message}"),
                install_prompt: CODEX_INSTALL_PROMPT.to_owned(),
                auth_setup: Some(auth_setup),
                checked_at,
                binary_path: Some(binary_path),
                requires_openai_auth: None,
                signed_in: false,
                auth_method: None,
                account_label: None,
                plan_type: None,
                payment_model: None,
                preconditions,
                rate_limits: Vec::new(),
                usage_summary: None,
                warnings,
            };
        }
    };

    let requires_openai_auth = bool_at(&account_value, &["requiresOpenaiAuth"]);
    let account = account_value
        .get("account")
        .filter(|value| !value.is_null());
    let auth_method = account.and_then(|account| string_at(account, &["type"]));
    let account_label = account_label(account, auth_method.as_deref());
    let signed_in = account.is_some();
    let mut account_ok = signed_in || requires_openai_auth == Some(false);
    let mut account_message = if signed_in {
        match (auth_method.as_deref(), account_label.as_deref()) {
            (Some("chatgpt"), Some(label)) => format!("Signed in with ChatGPT as {label}."),
            (Some("apiKey"), _) => "Signed in with an API key.".to_owned(),
            (Some(method), _) => format!("Signed in with {method}."),
            (None, _) => "Signed in.".to_owned(),
        }
    } else if requires_openai_auth == Some(false) {
        "The active Codex provider does not require OpenAI authentication.".to_owned()
    } else {
        format!(
            "No Codex account is signed in for Patchbay's managed Codex home ({}).",
            codex_home_dir().display(),
        )
    };
    let mut auth_failure = None::<String>;

    let rate_limit_value = match client
        .account_rate_limits_read()
        .await
        .and_then(|rate_limits| {
            serde_json::to_value(rate_limits)
                .map_err(codex_app_server_sdk::ClientError::Serialization)
        }) {
        Ok(value) => Some(value),
        Err(err) => {
            if let Some(message) = auth_failure_message("reading rate limits", &err) {
                auth_failure.get_or_insert(message);
            } else {
                warnings.push(format!("Codex rate limits could not be read: {err}"));
            }
            None
        }
    };
    let mut rate_limits = rate_limit_value
        .as_ref()
        .map(parse_rate_limits)
        .unwrap_or_default();
    rate_limits.sort_by(|left, right| left.label.cmp(&right.label));
    let plan_type = account
        .and_then(|account| string_at(account, &["planType"]))
        .or_else(|| {
            rate_limits
                .iter()
                .find_map(|limit| limit.plan_type.as_ref().cloned())
        });
    let reached = rate_limits
        .iter()
        .filter_map(|limit| limit.reached_type.as_deref())
        .collect::<Vec<_>>();

    let usage_summary = match client
        .send_raw_request(
            "account/usage/read",
            Value::Null,
            Some(Duration::from_secs(8)),
        )
        .await
    {
        Ok(value) => parse_usage_summary(&value),
        Err(err) => {
            if let Some(message) = auth_failure_message("reading token usage", &err) {
                auth_failure.get_or_insert(message);
            } else {
                warnings.push(format!(
                    "Codex token usage summary could not be read: {err}"
                ));
            }
            None
        }
    };

    if let Some(message) = auth_failure {
        account_ok = false;
        account_message = message;
    }
    let auth_setup = (!account_ok).then(|| codex_auth_setup(codex_binary));
    preconditions.push(CodexPreconditionView {
        name: "Codex account".to_owned(),
        ok: account_ok,
        message: account_message.clone(),
    });
    preconditions.push(CodexPreconditionView {
        name: "Codex usage limits".to_owned(),
        ok: reached.is_empty(),
        message: if reached.is_empty() {
            if rate_limits.is_empty() {
                "No active Codex rate-limit block was reported.".to_owned()
            } else {
                "Codex rate limits are available and no limit block is active.".to_owned()
            }
        } else {
            format!("Codex reports active limit block: {}.", reached.join(", "))
        },
    });

    let usable = preconditions.iter().all(|precondition| precondition.ok);
    let payment_model = payment_model(auth_method.as_deref(), plan_type.as_deref());
    let message = if usable {
        match payment_model.as_deref() {
            Some(payment_model) => format!("Codex SDK is usable for automation ({payment_model})."),
            None => "Codex SDK is usable for automation.".to_owned(),
        }
    } else {
        let failed = preconditions
            .iter()
            .find(|precondition| !precondition.ok)
            .map(|precondition| precondition.message.clone())
            .unwrap_or_else(|| "A Codex automation precondition failed.".to_owned());
        format!("Codex SDK is unusable for automation. {failed}")
    };

    CodexAppServerStatusView {
        available: true,
        usable,
        message,
        install_prompt: CODEX_INSTALL_PROMPT.to_owned(),
        auth_setup,
        checked_at,
        binary_path: Some(binary_path),
        requires_openai_auth,
        signed_in,
        auth_method,
        account_label,
        plan_type,
        payment_model,
        preconditions,
        rate_limits,
        usage_summary,
        warnings,
    }
}

async fn spawn_initialized_client(codex_binary: &Path) -> Result<CodexClient> {
    let mut config = stdio_config(codex_binary);
    config.env = codex_environment()?;
    let client = CodexClient::spawn_stdio(config)
        .await
        .context("failed to start Codex app-server")?;
    let init = InitializeParams::new(ClientInfo::new(
        CLIENT_NAME,
        CLIENT_TITLE,
        env!("CARGO_PKG_VERSION"),
    ));
    client
        .initialize(init)
        .await
        .context("Codex app-server rejected initialize")?;
    client
        .initialized()
        .await
        .context("Codex app-server rejected initialized notification")?;
    Ok(client)
}

fn unavailable_status(checked_at: String, message: String) -> CodexAppServerStatusView {
    CodexAppServerStatusView {
        available: false,
        usable: false,
        message: message.clone(),
        install_prompt: CODEX_INSTALL_PROMPT.to_owned(),
        auth_setup: None,
        checked_at,
        binary_path: None,
        requires_openai_auth: None,
        signed_in: false,
        auth_method: None,
        account_label: None,
        plan_type: None,
        payment_model: None,
        preconditions: vec![CodexPreconditionView {
            name: "Codex app-server".to_owned(),
            ok: false,
            message,
        }],
        rate_limits: Vec::new(),
        usage_summary: None,
        warnings: Vec::new(),
    }
}

pub fn operator_guidance(status: &CodexAppServerStatusView) -> Vec<String> {
    let mut lines = vec![status.message.clone()];
    if !status.available {
        lines.push(status.install_prompt.clone());
    }
    if let Some(setup) = &status.auth_setup {
        lines.push("Sign in with Patchbay's managed Codex home by running:".to_owned());
        lines.push(setup.login_command.clone());
        lines.push(setup.refresh_instruction.clone());
        lines.push(setup.api_key_instruction.clone());
    }
    lines
}

fn auth_failure_message(context: &str, error: &ClientError) -> Option<String> {
    let text = match error {
        ClientError::Rpc { error } => {
            let data = error
                .data
                .as_ref()
                .map(Value::to_string)
                .unwrap_or_default();
            format!("{} {data}", error.message)
        }
        _ => error.to_string(),
    };
    is_invalidated_auth_error(&text).then(|| {
        format!(
            "Codex credentials in Patchbay's managed Codex home ({}) were rejected while {context}. Log out from Patchbay, then sign in again with the managed CODEX_HOME.",
            codex_home_dir().display(),
        )
    })
}

fn is_invalidated_auth_error(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("401 unauthorized")
        || message.contains("refresh_token_invalidated")
        || message.contains("token_invalidated")
        || message.contains("session has ended")
        || message.contains("authentication token has been invalidated")
}

fn account_label(account: Option<&Value>, auth_method: Option<&str>) -> Option<String> {
    match auth_method {
        Some("chatgpt") => account.and_then(|account| string_at(account, &["email"])),
        Some("apiKey") => Some("API key".to_owned()),
        Some("amazonBedrock") => Some("Amazon Bedrock".to_owned()),
        Some(method) => Some(method.to_owned()),
        None => None,
    }
}

fn payment_model(auth_method: Option<&str>, plan_type: Option<&str>) -> Option<String> {
    match auth_method {
        Some("apiKey") => Some("per token (API key)".to_owned()),
        Some("chatgpt") => plan_type.map(|plan| match plan {
            "self_serve_business_usage_based" | "enterprise_cbp_usage_based" => {
                format!("usage-based ChatGPT workspace ({plan})")
            }
            "unknown" => "ChatGPT subscription (unknown plan)".to_owned(),
            plan => format!("ChatGPT subscription ({plan})"),
        }),
        Some(method) => Some(method.to_owned()),
        None => plan_type.map(|plan| format!("plan {plan}")),
    }
}

fn parse_rate_limits(value: &Value) -> Vec<CodexRateLimitView> {
    let mut limits = Vec::new();
    if let Some(by_id) = value
        .get("rateLimitsByLimitId")
        .and_then(Value::as_object)
        .filter(|by_id| !by_id.is_empty())
    {
        for (key, snapshot) in by_id {
            limits.push(parse_rate_limit_snapshot(snapshot, Some(key.as_str())));
        }
        return limits;
    }

    if let Some(snapshot) = value.get("rateLimits") {
        limits.push(parse_rate_limit_snapshot(snapshot, None));
    }
    limits
}

fn parse_rate_limit_snapshot(value: &Value, fallback_label: Option<&str>) -> CodexRateLimitView {
    let limit_id = string_at(value, &["limitId"]);
    let limit_name = string_at(value, &["limitName"]);
    let label = limit_name
        .or_else(|| limit_id.clone())
        .or_else(|| fallback_label.map(ToOwned::to_owned))
        .unwrap_or_else(|| "Codex".to_owned());
    let primary = value.get("primary");
    let secondary = value.get("secondary");
    let individual = value.get("individualLimit");
    let credits = value.get("credits");

    CodexRateLimitView {
        label,
        plan_type: string_at(value, &["planType"]),
        primary_used_percent: primary.and_then(|value| i64_at(value, &["usedPercent"])),
        primary_window_minutes: primary.and_then(|value| i64_at(value, &["windowDurationMins"])),
        primary_resets_at: primary
            .and_then(|value| i64_at(value, &["resetsAt"]))
            .and_then(format_unix_timestamp),
        secondary_used_percent: secondary.and_then(|value| i64_at(value, &["usedPercent"])),
        secondary_window_minutes: secondary
            .and_then(|value| i64_at(value, &["windowDurationMins"])),
        secondary_resets_at: secondary
            .and_then(|value| i64_at(value, &["resetsAt"]))
            .and_then(format_unix_timestamp),
        individual_used: individual.and_then(|value| string_at(value, &["used"])),
        individual_limit: individual.and_then(|value| string_at(value, &["limit"])),
        individual_remaining_percent: individual
            .and_then(|value| i64_at(value, &["remainingPercent"])),
        individual_resets_at: individual
            .and_then(|value| i64_at(value, &["resetsAt"]))
            .and_then(format_unix_timestamp),
        credits_balance: credits.and_then(|value| string_at(value, &["balance"])),
        credits_has_credits: credits.and_then(|value| bool_at(value, &["hasCredits"])),
        credits_unlimited: credits.and_then(|value| bool_at(value, &["unlimited"])),
        reached_type: string_at(value, &["rateLimitReachedType"]),
    }
}

fn parse_usage_summary(value: &Value) -> Option<CodexUsageSummaryView> {
    let summary = value.get("summary")?;
    Some(CodexUsageSummaryView {
        lifetime_tokens: i64_at(summary, &["lifetimeTokens"]),
        peak_daily_tokens: i64_at(summary, &["peakDailyTokens"]),
        current_streak_days: i64_at(summary, &["currentStreakDays"]),
        longest_streak_days: i64_at(summary, &["longestStreakDays"]),
        longest_running_turn_seconds: i64_at(summary, &["longestRunningTurnSec"]),
    })
}

fn string_at(value: &Value, path: &[&str]) -> Option<String> {
    path.iter()
        .try_fold(value, |current, key| current.get(*key))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn bool_at(value: &Value, path: &[&str]) -> Option<bool> {
    path.iter()
        .try_fold(value, |current, key| current.get(*key))
        .and_then(Value::as_bool)
}

fn i64_at(value: &Value, path: &[&str]) -> Option<i64> {
    path.iter()
        .try_fold(value, |current, key| current.get(*key))
        .and_then(Value::as_i64)
}

fn format_unix_timestamp(timestamp: i64) -> Option<String> {
    OffsetDateTime::from_unix_timestamp(timestamp)
        .ok()
        .and_then(|time| time.format(&Rfc3339).ok())
}

pub async fn spawn_codex_with_home_and_env(
    codex_binary: &Path,
    codex_home: &Path,
    mut env: HashMap<String, String>,
) -> Result<Codex> {
    let mut config = stdio_config(codex_binary);
    env.extend(codex_environment_for_home(codex_home)?);
    config.env = env;
    Ok(Codex::spawn_stdio(config)
        .await
        .context("failed to start Codex app-server")?)
}

pub fn codex_home_dir() -> PathBuf {
    codex_home_dir_for_patchbay_home(&patchbay_home_dir())
}

pub fn codex_config_path() -> PathBuf {
    codex_config_path_for_home(&codex_home_dir())
}

pub fn codex_project_home_dir(project_id: i64) -> PathBuf {
    codex_home_dir()
        .join("projects")
        .join(project_id.to_string())
}

pub fn ensure_project_codex_home(settings: &ProjectSettingsView) -> Result<PathBuf> {
    let shared_home = ensure_codex_home()?;
    let project_home = codex_project_home_dir(settings.project_id);
    fs::create_dir_all(&project_home).context_with(|| {
        format!(
            "failed to create Patchbay project Codex home {}",
            project_home.display()
        )
    })?;
    link_shared_codex_entry(&shared_home, &project_home, "auth.json")?;
    link_shared_codex_entry(&shared_home, &project_home, "installation_id")?;
    link_shared_codex_entry(&shared_home, &project_home, "skills")?;
    write_project_codex_config(&project_home, settings)?;
    write_project_git_rules(&project_home, settings)?;
    Ok(project_home)
}

fn codex_auth_setup(codex_binary: &Path) -> CodexAuthSetupView {
    let codex_home = codex_home_dir();
    let codex_config = codex_config_path_for_home(&codex_home);
    CodexAuthSetupView {
        codex_home_path: codex_home.to_string_lossy().into_owned(),
        codex_config_path: codex_config.to_string_lossy().into_owned(),
        login_command: codex_login_command_for(codex_binary, &codex_home),
        refresh_instruction:
            "After the browser login completes, return to Patchbay. Patchbay refreshes this state automatically; Refresh checks it immediately.".to_owned(),
        api_key_instruction:
            "For API-key auth instead, start the Patchbay server with OPENAI_API_KEY set."
                .to_owned(),
    }
}

fn codex_login_command_for(codex_binary: &Path, codex_home: &Path) -> String {
    let codex_home = codex_home.to_string_lossy();
    let codex_binary = codex_binary.to_string_lossy();
    format!(
        "CODEX_HOME={} CODEX_SQLITE_HOME={} {} login",
        shell_quote(codex_home.as_ref()),
        shell_quote(codex_home.as_ref()),
        shell_quote(codex_binary.as_ref()),
    )
}

fn shell_quote(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-' | ':'))
    {
        return value.to_owned();
    }

    format!("'{}'", value.replace('\'', "'\\''"))
}

fn codex_environment() -> Result<HashMap<String, String>> {
    let codex_home = ensure_codex_home()?;
    codex_environment_for_home(&codex_home)
}

fn codex_environment_for_home(codex_home: &Path) -> Result<HashMap<String, String>> {
    if !codex_home.is_dir() {
        bail!(
            "Patchbay Codex home {} does not exist or is not a directory",
            codex_home.display()
        );
    }
    let codex_home = codex_home.to_string_lossy().into_owned();
    Ok(HashMap::from([
        ("CODEX_HOME".to_owned(), codex_home.clone()),
        ("CODEX_SQLITE_HOME".to_owned(), codex_home),
    ]))
}

fn ensure_codex_home() -> Result<PathBuf> {
    let codex_home = codex_home_dir();
    ensure_codex_home_at(&codex_home)?;
    Ok(codex_home)
}

fn codex_home_dir_for_patchbay_home(patchbay_home: &Path) -> PathBuf {
    patchbay_home.join(CODEX_HOME_DIR)
}

fn codex_config_path_for_home(codex_home: &Path) -> PathBuf {
    codex_home.join("config.toml")
}

fn ensure_codex_home_at(codex_home: &Path) -> Result<()> {
    fs::create_dir_all(codex_home).context_with(|| {
        format!(
            "failed to create Patchbay Codex home {}",
            codex_home.display()
        )
    })?;
    let config_path = codex_config_path_for_home(codex_home);
    fs::write(&config_path, CODEX_CONFIG)
        .context_with(|| format!("failed to write Codex config {}", config_path.display()))?;
    Ok(())
}

fn link_shared_codex_entry(
    shared_home: &Path,
    project_home: &Path,
    entry_name: &str,
) -> Result<()> {
    let source = shared_home.join(entry_name);
    if !source.exists() {
        return Ok(());
    }
    let destination = project_home.join(entry_name);
    if fs::symlink_metadata(&destination).is_ok() {
        return Ok(());
    }
    Ok(symlink_path(&source, &destination).context_with(|| {
        format!(
            "failed to link shared Codex entry {} into {}",
            source.display(),
            destination.display()
        )
    })?)
}

#[cfg(unix)]
fn symlink_path(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(source, destination)
}

#[cfg(windows)]
fn symlink_path(source: &Path, destination: &Path) -> std::io::Result<()> {
    if source.is_dir() {
        std::os::windows::fs::symlink_dir(source, destination)
    } else {
        std::os::windows::fs::symlink_file(source, destination)
    }
}

fn write_project_codex_config(codex_home: &Path, settings: &ProjectSettingsView) -> Result<()> {
    let config_path = codex_config_path_for_home(codex_home);
    let sandbox_mode = match settings.agent_sandbox_mode {
        AgentSandboxMode::WorkspaceWrite => "workspace-write",
        AgentSandboxMode::DangerFullAccess => "danger-full-access",
    };
    let writable_roots = toml_string_array(&settings.agent_extra_writable_roots);
    let config = format!(
        r#"# Managed by Patchbay.
# This file is regenerated from the Patchbay project settings.

approval_policy = "never"
sandbox_mode = {sandbox_mode}

[features]
memories = false

[memories]
use_memories = false
generate_memories = false
disable_on_external_context = true

[sandbox_workspace_write]
network_access = true
writable_roots = {writable_roots}
"#,
        sandbox_mode = toml_string(sandbox_mode),
    );
    Ok(fs::write(&config_path, config)
        .context_with(|| format!("failed to write Codex config {}", config_path.display()))?)
}

fn write_project_git_rules(codex_home: &Path, settings: &ProjectSettingsView) -> Result<()> {
    let rules_dir = codex_home.join("rules");
    fs::create_dir_all(&rules_dir)
        .context_with(|| format!("failed to create Codex rules dir {}", rules_dir.display()))?;
    let rules_path = rules_dir.join(PROJECT_RULES_FILE_NAME);
    Ok(
        fs::write(&rules_path, git_rules_for_policy(settings)).context_with(|| {
            format!(
                "failed to write Codex git rules file {}",
                rules_path.display()
            )
        })?,
    )
}

fn git_rules_for_policy(settings: &ProjectSettingsView) -> String {
    let policy = &settings.agent_git_command_policy;
    let mut rules = String::from(
        r#"# Managed by Patchbay.
# These rules mirror this project's Patchbay Git command policy. Patchbay also
# puts a guarded git shim first on PATH for checks that command prefixes cannot express.

"#,
    );
    if policy.add {
        rules.push_str(&prefix_rule(
            &["git", "add"],
            "allow",
            "Patchbay allows staging explicit changes for this project.",
        ));
    }
    if policy.commit {
        rules.push_str(&prefix_rule(
            &["git", "commit"],
            "allow",
            "Patchbay allows commits for this project; the git wrapper enforces --no-verify.",
        ));
    }
    if policy.push {
        rules.push_str(&prefix_rule(
            &["git", "push"],
            "allow",
            "Patchbay allows non-force pushes for this project.",
        ));
        for forbidden in [
            ["git", "push", "--force"],
            ["git", "push", "-f"],
            ["git", "push", "--force-with-lease"],
            ["git", "push", "--mirror"],
            ["git", "push", "--delete"],
            ["git", "push", "--prune"],
        ] {
            rules.push_str(&prefix_rule(
                &forbidden,
                "forbidden",
                "Patchbay blocks force, mirror, delete, and prune pushes; use a normal git push.",
            ));
        }
    }
    if policy.reset {
        rules.push_str(&prefix_rule(
            &["git", "reset"],
            "allow",
            "Patchbay allows git reset within this project's configured limits.",
        ));
        if !policy.allows_hard_reset(settings.workspace_mode) {
            rules.push_str(&prefix_rule(
                &["git", "reset", "--hard"],
                "forbidden",
                "Patchbay blocks git reset --hard for the current workspace mode.",
            ));
        }
    }
    rules
}

fn prefix_rule(pattern: &[&str], decision: &str, justification: &str) -> String {
    let pattern = pattern
        .iter()
        .map(|value| toml_string(value))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        r#"prefix_rule(
    pattern = [{pattern}],
    decision = {decision},
    justification = {justification},
)

"#,
        decision = toml_string(decision),
        justification = toml_string(justification),
    )
}

fn toml_string_array(values: &[String]) -> String {
    let values = values
        .iter()
        .map(|value| toml_string(value))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{values}]")
}

fn toml_string(value: &str) -> String {
    serde_json::to_string(value).expect("TOML-compatible string must serialize")
}

fn stdio_config(codex_binary: &Path) -> StdioConfig {
    StdioConfig {
        codex_binary: codex_binary.to_string_lossy().into_owned(),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use tempfile::TempDir;

    use super::*;
    use crate::backend::{agent_tools::set_tool_path, storage::Store};
    use crate::shared::view_models::{
        AgentGitCommandPolicy, AgentReasoningEffort, RevertStrategy, WorkspaceMode,
        WorktreeCleanupPolicy,
    };

    #[tokio::test]
    async fn unavailable_app_server_status_prompts_for_codex_install() {
        let temp = TempDir::new().unwrap();
        let store = Store::open(temp.path().join("patchbay.sqlite3"))
            .await
            .unwrap();
        set_tool_path(
            &store,
            AgentToolName::Codex,
            PathBuf::from("/definitely/missing/codex"),
        )
        .await
        .unwrap();

        let status = app_server_status(&store).await;

        assert!(!status.available);
        assert!(status.message.contains("Codex app-server is unavailable"));
        assert!(status.install_prompt.contains("Install Codex"));
    }

    #[test]
    fn codex_login_command_uses_managed_home_and_binary() {
        let command = codex_login_command_for(
            Path::new("/opt/codex/bin/codex"),
            Path::new("/Users/test/.patchbay/codex"),
        );

        assert_eq!(
            command,
            "CODEX_HOME=/Users/test/.patchbay/codex CODEX_SQLITE_HOME=/Users/test/.patchbay/codex /opt/codex/bin/codex login"
        );
    }

    #[test]
    fn codex_login_command_quotes_shell_values() {
        let command = codex_login_command_for(
            Path::new("/Applications/Codex CLI/codex"),
            Path::new("/Users/test/Patchbay Home/codex"),
        );

        assert_eq!(
            command,
            "CODEX_HOME='/Users/test/Patchbay Home/codex' CODEX_SQLITE_HOME='/Users/test/Patchbay Home/codex' '/Applications/Codex CLI/codex' login"
        );
    }

    #[test]
    fn operator_guidance_for_auth_setup_omits_install_prompt() {
        let status = CodexAppServerStatusView {
            available: true,
            usable: false,
            message: "Codex SDK is unusable for automation.".to_owned(),
            install_prompt: CODEX_INSTALL_PROMPT.to_owned(),
            auth_setup: Some(CodexAuthSetupView {
                codex_home_path: "/Users/test/.patchbay/codex".to_owned(),
                codex_config_path: "/Users/test/.patchbay/codex/config.toml".to_owned(),
                login_command: "CODEX_HOME=/Users/test/.patchbay/codex codex login".to_owned(),
                refresh_instruction: "Refresh after login.".to_owned(),
                api_key_instruction: "Set OPENAI_API_KEY.".to_owned(),
            }),
            ..Default::default()
        };

        let guidance = operator_guidance(&status).join("\n");

        assert!(guidance.contains("CODEX_HOME=/Users/test/.patchbay/codex codex login"));
        assert!(!guidance.contains(CODEX_INSTALL_PROMPT));
    }

    #[test]
    fn invalidated_auth_errors_are_account_failures() {
        let error = ClientError::Rpc {
            error: codex_app_server_sdk::RpcError {
                code: -32603,
                message: "failed to fetch codex rate limits: 401 Unauthorized token_invalidated"
                    .to_owned(),
                data: None,
            },
        };

        let message = auth_failure_message("reading rate limits", &error).unwrap();

        assert!(message.contains("managed Codex home"));
        assert!(message.contains("Log out"));
    }

    #[test]
    fn codex_home_config_disables_memories() {
        let temp = TempDir::new().unwrap();
        let codex_home = codex_home_dir_for_patchbay_home(temp.path());

        ensure_codex_home_at(&codex_home).unwrap();

        let config = fs::read_to_string(codex_config_path_for_home(&codex_home)).unwrap();
        assert!(config.contains("[features]"));
        assert!(config.contains("memories = false"));
        assert!(config.contains("[memories]"));
        assert!(config.contains("use_memories = false"));
        assert!(config.contains("generate_memories = false"));
    }

    #[test]
    fn project_codex_config_uses_project_sandbox_settings() {
        let temp = TempDir::new().unwrap();
        let mut settings = project_settings();
        settings.agent_extra_writable_roots = vec!["/tmp/patchbay-browser".to_owned()];

        write_project_codex_config(temp.path(), &settings).unwrap();

        let config = fs::read_to_string(codex_config_path_for_home(temp.path())).unwrap();
        assert!(config.contains("approval_policy = \"never\""));
        assert!(config.contains("sandbox_mode = \"workspace-write\""));
        assert!(config.contains("network_access = true"));
        assert!(config.contains("writable_roots = [\"/tmp/patchbay-browser\"]"));
        assert!(config.contains("memories = false"));
    }

    #[test]
    fn project_git_rules_reflect_allowed_commands_and_hard_reset() {
        let mut settings = project_settings();
        settings.workspace_mode = WorkspaceMode::CurrentBranch;
        settings.agent_git_command_policy = AgentGitCommandPolicy {
            add: true,
            commit: true,
            push: true,
            reset: true,
            ..Default::default()
        };

        let rules = git_rules_for_policy(&settings);

        assert!(rules.contains("pattern = [\"git\", \"add\"]"));
        assert!(rules.contains("pattern = [\"git\", \"commit\"]"));
        assert!(rules.contains("pattern = [\"git\", \"push\"]"));
        assert!(rules.contains("pattern = [\"git\", \"push\", \"--force\"]"));
        assert!(rules.contains("pattern = [\"git\", \"reset\"]"));
        assert!(rules.contains("pattern = [\"git\", \"reset\", \"--hard\"]"));
        assert!(rules.contains("decision = \"forbidden\""));
    }

    fn project_settings() -> ProjectSettingsView {
        ProjectSettingsView {
            id: 7,
            project_id: 7,
            workspace_mode: WorkspaceMode::GitWorktree,
            max_code_edit_agents: 1,
            max_read_only_agents: 2,
            create_pr: false,
            auto_commit: true,
            commit_standard: String::new(),
            revert_strategy: RevertStrategy::Manual,
            stale_claim_minutes: 0,
            worktree_cleanup_policy: WorktreeCleanupPolicy::Manual,
            default_agent_tool: AgentToolName::Codex,
            default_agent_model: None,
            default_agent_reasoning_effort: Some(AgentReasoningEffort::XHigh),
            agent_sandbox_mode: AgentSandboxMode::WorkspaceWrite,
            agent_extra_writable_roots: Vec::new(),
            agent_git_command_policy: AgentGitCommandPolicy::default(),
            created_at: "2026-06-17T00:00:00Z".to_owned(),
            updated_at: "2026-06-17T00:00:00Z".to_owned(),
        }
    }
}
