#![cfg(not(target_arch = "wasm32"))]

use std::{borrow::Cow, env, fs, path::Path, time::Duration};

use assertr::prelude::*;
use browser_test::thirtyfour::{By, ChromiumLikeCapabilities, Key, WebDriver};
use browser_test::{
    BrowserTest, BrowserTestFailurePolicy, BrowserTestParallelism, BrowserTestRunner,
    BrowserTestVisibility, BrowserTests, BrowserTimeouts, ChromeBinary, PauseConfig, async_trait,
};
use leptos_browser_test::{LeptosTestApp, LeptosTestAppConfig, Report, ResultExt, bail};
use tempfile::TempDir;

#[tokio::test(flavor = "multi_thread")]
async fn browser_tests() -> Result<(), Report> {
    tracing_subscriber::fmt().init();

    let app = PatchbayTestApp::start().await?;
    let browser_visibility = BrowserTestVisibility::from_env();
    let run_chrome_single_process = browser_visibility.resolve().is_headless();

    let run_result = BrowserTestRunner::new()
        // Headless Shell is a command-line Chrome-for-Testing artifact, not the macOS Chrome .app
        // bundle. That avoids LaunchServices / WindowServer app-registration calls that are
        // blocked by the default Codex SDK sandbox before WebDriver can create a session. Visible
        // browser-test runs still use regular Chrome because Headless Shell cannot show a window.
        .with_headless_chrome_binary(ChromeBinary::ChromeHeadlessShell)
        .with_chrome_capabilities(move |caps| {
            // Chrome's process sandbox can fail in nested/managed CI-style sandboxes. WebDriver
            // still runs in Patchbay's test process sandbox, so this only disables Chrome's own
            // child-process sandbox layer.
            caps.add_arg("--no-sandbox")?;
            if run_chrome_single_process {
                // The Codex SDK workspace sandbox on macOS denies Mach service registration. In
                // Headless Shell, Chromium otherwise registers
                // org.chromium.Chromium.MachPortRendezvousServer.<pid> before DevTools startup for
                // child-process rendezvous. Keeping the headless browser in one process avoids that
                // bootstrap_check_in path; visible debugging runs stay multi-process.
                caps.add_arg("--single-process")?;
            }
            // Avoid /dev/shm startup failures in restricted environments by using regular temp
            // files for Chrome IPC/shared-memory storage.
            caps.add_arg("--disable-dev-shm-usage")?;
            Ok(())
        })
        .with_test_parallelism(BrowserTestParallelism::Sequential)
        .with_failure_policy(BrowserTestFailurePolicy::RunAll)
        .with_visibility(browser_visibility)
        .with_pause(PauseConfig::from_env())
        .with_timeouts(
            BrowserTimeouts::builder()
                .implicit_wait_timeout(Duration::from_secs(10))
                .page_load_timeout(Duration::from_secs(20))
                .build(),
        )
        .run(&app, BrowserTests::new().with(PatchbayBoardTest))
        .await;

    run_result.map_err(Report::into_dynamic)?;

    Ok(())
}

struct PatchbayTestApp {
    _app: LeptosTestApp,
    _tmpdir: TempDir,
    base_url: String,
}

impl PatchbayTestApp {
    async fn start() -> Result<Self, Report> {
        let tmpdir =
            tempfile::tempdir().context("failed to create Patchbay browser-test temp dir")?;
        let database = tmpdir.path().join("patchbay.sqlite3");
        let editor_bin_dir = tmpdir.path().join("bin");
        fs::create_dir(&editor_bin_dir).context("failed to create browser-test editor bin dir")?;
        for program in [
            "rustrover",
            "rustrover64.exe",
            "code",
            "code.cmd",
            "code.exe",
        ] {
            write_test_executable(&editor_bin_dir.join(program))?;
        }
        let test_path = path_with_prefix(&editor_bin_dir)?;

        let app = LeptosTestAppConfig::new(env!("CARGO_MANIFEST_DIR"))
            .with_app_name("patchbay browser test")
            .with_forward_logs(true)
            .with_startup_line("Serving Patchbay")
            .with_env("PATCHBAY_DATABASE", database.as_os_str())
            .with_env("PATH", test_path.as_os_str())
            .start()
            .await
            .map_err(Report::into_dynamic)?;

        let base_url = app.base_url().to_owned();

        Ok(Self {
            _app: app,
            _tmpdir: tmpdir,
            base_url,
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }
}

fn path_with_prefix(prefix: &Path) -> Result<std::ffi::OsString, Report> {
    let mut entries = vec![prefix.to_path_buf()];
    if let Some(path) = env::var_os("PATH") {
        entries.extend(env::split_paths(&path));
    }
    Ok(env::join_paths(entries).context("failed to build browser-test PATH")?)
}

fn write_test_executable(path: &Path) -> Result<(), Report> {
    fs::write(path, "#!/bin/sh\nexit 0\n").context("failed to write browser-test editor shim")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = fs::metadata(path)
            .context("failed to stat browser-test editor shim")?
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions)
            .context("failed to make browser-test editor shim executable")?;
    }
    Ok(())
}

struct PatchbayBoardTest;

#[async_trait]
impl BrowserTest<PatchbayTestApp> for PatchbayBoardTest {
    fn name(&self) -> Cow<'_, str> {
        Cow::Borrowed("patchbay board renders and creates work")
    }

    async fn run(&self, driver: &WebDriver, app: &PatchbayTestApp) -> Result<(), Report> {
        driver
            .goto(app.url("/projects"))
            .await
            .context("failed to open Patchbay projects page")?;

        assert_that!(driver.title().await.context("failed to read page title")?)
            .is_equal_to("Projects");
        find(driver, By::Css(".project-switcher")).await?;
        find(driver, By::Css("[data-crudkit-leptos='projects']")).await?;
        assert_source_contains(driver, "project-switcher").await?;
        assert_source_does_not_contain(driver, ">Switch<").await?;
        assert_source_contains(driver, "data-crudkit-leptos=\"projects\"").await?;
        assert_source_does_not_contain(driver, "Existing projects").await?;
        assert_source_does_not_contain(driver, "project-create-form").await?;
        assert_source_contains(driver, "Codex app-server").await?;
        find(driver, By::Css(".topbar-codex")).await?;
        assert_source_does_not_contain(driver, "codex-status-panel").await?;
        click(driver, By::Css(".topbar-codex")).await?;
        assert_that!(
            driver
                .title()
                .await
                .context("failed to read Codex page title")?
        )
        .is_equal_to("Codex automation");
        find(driver, By::Css(".codex-status-panel")).await?;
        assert_codex_auth_guide_when_blocked(driver).await?;
        driver
            .goto(app.url("/projects"))
            .await
            .context("failed to reopen Patchbay projects page after Codex status check")?;
        assert_source_contains(driver, "data-crudkit-leptos=\"agent-tools\"").await?;
        assert_source_does_not_contain(driver, "/agent-tools/create").await?;
        find(
            driver,
            By::Css("[data-crudkit-leptos='projects'] .crud-nav"),
        )
        .await?;
        click(
            driver,
            By::Css("[data-crudkit-leptos='projects'] .crud-nav button"),
        )
        .await?;
        find(
            driver,
            By::Css("[data-crudkit-leptos='projects'] select.agent-model-select"),
        )
        .await?;
        find(
            driver,
            By::Css("[data-crudkit-leptos='projects'] select.agent-reasoning-select"),
        )
        .await?;
        find(
            driver,
            By::Css("[data-crudkit-leptos='projects'] option[value='gpt-5.5']"),
        )
        .await?;
        find(
            driver,
            By::Css("[data-crudkit-leptos='projects'] option[value='xhigh']"),
        )
        .await?;
        driver
            .goto(app.url("/projects"))
            .await
            .context("failed to reopen Patchbay projects page after create-view check")?;
        find(
            driver,
            By::Css("[data-crudkit-leptos='projects'] .crud-nav"),
        )
        .await?;
        find(
            driver,
            By::Css("[data-crudkit-leptos='agent-tools'] .crud-nav"),
        )
        .await?;
        assert_source_does_not_contain(driver, "Invalid URL").await?;
        assert_source_does_not_contain(driver, "relative URL without a base").await?;

        create_project(driver).await?;
        create_alternate_project(driver).await?;
        seed_system_prompt_history(driver).await?;
        seed_memory_history(driver).await?;
        driver
            .goto(app.url("/?project=demo"))
            .await
            .context("failed to open Patchbay board page")?;

        find(driver, By::Css("section.project-settings")).await?;
        find(driver, By::Css("section.board")).await?;
        assert_board_shell_uses_viewport_width(driver).await?;
        find(driver, By::Css(".workspace-panel .workspace-actions")).await?;
        assert_that!(driver.title().await.context("failed to read page title")?)
            .is_equal_to("Patchbay");
        assert_source_contains(driver, "Copy path").await?;
        assert_source_does_not_contain(driver, "Copy cd").await?;
        assert_source_contains(driver, "Open folder").await?;
        assert_source_contains(driver, "Open RustRover").await?;
        assert_source_contains(driver, "Open VS Code").await?;
        find(
            driver,
            By::Css("img.workspace-button-icon[src=\"/icons/workspace-rustrover.svg\"]"),
        )
        .await?;
        find(
            driver,
            By::Css("img.workspace-button-icon[src=\"/icons/workspace-vscode.svg\"]"),
        )
        .await?;
        assert_source_contains(driver, "Git repository").await?;
        find(driver, By::Css(".workspace-git-status")).await?;
        find(driver, By::Css(".workspace-git-diff")).await?;
        assert_source_does_not_contain(driver, "Open IDE").await?;
        assert_source_contains(driver, "System prompt").await?;
        assert_source_does_not_contain(driver, "project-option-key").await?;
        assert_source_contains(driver, "Memory").await?;
        assert_source_contains(driver, "Automation policy").await?;
        assert_source_contains(driver, "Read-only agents").await?;
        find(driver, By::Css("#project-max-read-only-agents")).await?;
        assert_source_contains(driver, "Auto-Commit").await?;
        find(driver, By::Css("#project-auto-commit")).await?;
        find(driver, By::Css("#project-commit-standard")).await?;
        find(
            driver,
            By::Css("#project-revert-strategy option[value='git_reset']"),
        )
        .await?;
        assert_source_contains(driver, "system prompt history").await?;
        assert_source_contains(driver, "memory history").await?;
        assert_source_does_not_contain(driver, "Compact history").await?;
        assert_source_does_not_contain(driver, "Append memory").await?;
        assert_source_does_not_contain(driver, "append-memory").await?;
        assert_source_does_not_contain(driver, "/memory/append").await?;
        assert_source_does_not_contain(driver, "memory-history-entry").await?;
        assert_source_does_not_contain(driver, "memory-snapshot").await?;
        assert_source_does_not_contain(driver, "Allow refinement while editing").await?;
        assert_settings_response_omits_refinement_policy(driver).await?;
        find(driver, By::Css("#project-system-prompt-version")).await?;
        find(driver, By::Css("textarea.project-system-prompt-text")).await?;
        assert_system_prompt_history_selector_behaviour(driver).await?;
        find(driver, By::Css("#project-memory-version")).await?;
        find(driver, By::Css("textarea.project-memory-text")).await?;
        assert_memory_history_selector_behaviour(driver).await?;
        assert_source_does_not_contain(driver, "Run settings").await?;
        assert_top_nav_order(driver).await?;
        find(driver, By::Css(".top-nav a[href='/runs?project=demo']")).await?;
        assert_source_does_not_contain(driver, "No runs yet").await?;
        assert_source_does_not_contain(driver, "CrudKit resources").await?;
        find(driver, By::Css(".topbar-codex")).await?;
        assert_source_does_not_contain(driver, "codex-status-panel").await?;
        find(driver, By::Css(".topbar-auto-commit[role='switch']")).await?;
        assert_auto_commit_toggle_updates_without_navigation(driver).await?;
        find(driver, By::Css(".topbar-automation button")).await?;
        assert_source_contains(driver, "Stopped").await?;
        assert_source_does_not_contain(driver, "Start automation").await?;
        assert_source_does_not_contain(driver, "Recover stale claims").await?;
        assert_source_contains(driver, "Maintenance").await?;
        assert_source_contains(driver, "Cleanup worktrees").await?;
        assert_source_contains(driver, "data-crudkit-leptos=\"work-items\"").await?;
        assert_source_does_not_contain(driver, "data-crudkit-leptos=\"swim-lanes\"").await?;
        assert_source_does_not_contain(driver, "data-crudkit-leptos=\"work-item-states\"").await?;
        assert_source_does_not_contain(driver, "Deserialize(").await?;
        assert_source_does_not_contain(driver, "missing field `identifier`").await?;
        assert_source_does_not_contain(driver, "unknown variant `Position`").await?;
        find(driver, By::Css(".lane:nth-child(1) .lane-edit")).await?;
        find(driver, By::Css(".lane:nth-child(1) .lane-add")).await?;
        find(driver, By::Css(".lane:nth-child(2) .lane-add")).await?;
        assert_lane_add_button_count(driver, 2).await?;
        assert_source_does_not_contain(driver, "data-crudkit-leptos=\"automation-triggers\"")
            .await?;
        assert_cached_frontend_route_revisit_avoids_loading(driver).await?;
        assert_crudkit_create_form_survives_live_event(driver).await?;
        assert_request_error_toast_preserves_draft(driver).await?;

        driver
            .goto(app.url("/projects?project=demo"))
            .await
            .context("failed to open Patchbay projects page for workflow authoring")?;
        find(
            driver,
            By::Css("[data-crudkit-leptos='work-item-states'] .crud-nav"),
        )
        .await?;
        find(
            driver,
            By::Css("[data-crudkit-leptos='swim-lanes'] .crud-nav"),
        )
        .await?;
        assert_source_contains(driver, "data-crudkit-leptos=\"work-item-states\"").await?;
        assert_source_contains(driver, "data-crudkit-leptos=\"swim-lanes\"").await?;

        driver
            .goto(app.url("/runs?project=demo"))
            .await
            .context("failed to open Patchbay runs page")?;
        assert_that!(driver.title().await.context("failed to read page title")?)
            .is_equal_to("Runs");
        find(driver, By::Css(".runs-page .automation")).await?;
        find(
            driver,
            By::Css(".top-nav a.active[href='/runs?project=demo']"),
        )
        .await?;
        assert_source_contains(driver, "No runs yet").await?;
        assert_source_contains(driver, "0 running (0 mutating, 0 read-only)").await?;
        assert_source_does_not_contain(driver, "data-crudkit-leptos=\"automation-triggers\"")
            .await?;

        driver
            .goto(app.url("/automation?project=demo"))
            .await
            .context("failed to open Patchbay automation page")?;
        assert_that!(driver.title().await.context("failed to read page title")?)
            .is_equal_to("Automation");
        find(
            driver,
            By::Css("[data-crudkit-leptos='automation-triggers'] .crud-nav"),
        )
        .await?;
        find(driver, By::Css(".trigger-runs")).await?;
        assert_source_contains(driver, "data-crudkit-leptos=\"automation-triggers\"").await?;
        assert_source_contains(driver, "Work-consuming automations").await?;
        assert_source_contains(driver, "Work-producing automations").await?;
        assert_source_contains(driver, "Mutability").await?;
        assert_source_contains(driver, "No automation selected").await?;
        assert_source_does_not_contain(driver, "Create trigger").await?;
        assert_source_does_not_contain(driver, "trigger-edit-form").await?;

        create_trigger(driver).await?;
        driver
            .goto(app.url("/automation?project=demo"))
            .await
            .context("failed to reload Patchbay automation page after automation creation")?;
        find(
            driver,
            By::Css("[data-crudkit-leptos='automation-triggers'] .crud-nav"),
        )
        .await?;
        assert_source_contains(driver, "refine-new").await?;

        driver
            .goto(app.url("/?project=demo"))
            .await
            .context("failed to reopen Patchbay board page")?;
        assert_source_does_not_contain(driver, "Patchbay labels").await?;
        driver
            .goto(app.url("/api/docs?project=demo"))
            .await
            .context("failed to open Patchbay API page")?;
        find(driver, By::Css("section.patchbay-labels")).await?;
        assert_source_contains(driver, "patchbay:automation-blocked").await?;
        assert_source_contains(driver, "patchbay:feedback-requested").await?;
        driver
            .goto(app.url("/?project=demo"))
            .await
            .context("failed to reopen Patchbay board page after API check")?;
        open_new_item_modal(driver).await?;
        assert_new_item_modal_actions(driver).await?;
        find(driver, By::Css("#new-item-modal select[name='state']")).await?;
        assert_new_item_lane_options(driver).await?;
        close_clean_new_item_modal(driver).await?;
        assert_lane_add_preselects_state(driver).await?;
        assert_new_item_modal_dirty_leave_protection(driver).await?;
        open_new_item_modal(driver).await?;
        find(driver, By::Css("#new-item-modal select.agent-model-select")).await?;
        assert_source_contains(driver, "Project default").await?;
        set_input_value(driver, "#new-item-modal .crud-input-field", "Browser item").await?;
        set_input_value(
            driver,
            "#new-item-modal input[name='description']",
            "Created through browser-test\nSecond line",
        )
        .await?;
        append_new_item_initial_label(driver, "area", "browser").await?;
        append_new_item_initial_label(driver, "needs-verification", "").await?;
        click_new_item_save(driver).await?;

        find(driver, By::LinkText("Browser item")).await?;
        assert_board_card_contains(driver, "Browser item", "area=browser").await?;
        assert_board_card_contains(driver, "Browser item", "needs-verification").await?;
        assert_source_contains(driver, "Created through browser-test").await?;
        assert_source_contains(driver, "state=idea").await?;

        click(driver, By::LinkText("Browser item")).await?;
        find(driver, By::Css("section.item-settings")).await?;
        find(driver, By::Css("section.comments")).await?;
        assert_source_contains(driver, "Item details").await?;
        assert_source_contains(driver, "area=browser").await?;
        assert_source_contains(driver, "needs-verification").await?;
        assert_item_detail_description_is_not_duplicated(driver).await?;
        assert_item_detail_description_editor_accepts_click_and_text(driver).await?;
        let relationship_target_id = create_relationship_target_item(driver).await?;
        assert_item_relationship_create_delete_flow(driver, relationship_target_id).await?;
        assert_item_detail_dirty_leave_protection(driver).await?;
        click(driver, By::LinkText("Browser item")).await?;
        find(driver, By::Css("section.item-settings")).await?;
        assert_source_does_not_contain(driver, "automation can claim this item").await?;
        assert_source_does_not_contain(driver, "Set state").await?;
        find(
            driver,
            By::XPath(
                "//section[contains(@class, 'item-settings')]//button[contains(., 'Löschen')]",
            ),
        )
        .await?;
        assert_source_does_not_contain(driver, "Start agent").await?;
        assert_source_contains(driver, "Comments").await?;
        add_agent_comment(driver).await?;
        claim_current_item(driver).await?;
        let item_url = driver
            .current_url()
            .await
            .context("failed to read item URL after adding agent comment")?;
        driver
            .goto(item_url.as_str())
            .await
            .context("failed to reload item page after adding agent comment")?;
        find(
            driver,
            By::Css(
                "section.comments .comment-author-link[href='/projects/demo/automation/runs/60/log']",
            ),
        )
        .await?;
        find(
            driver,
            By::Css(".item-meta a.claim-badge[href='/projects/demo/automation/runs/60/log']"),
        )
        .await?;
        assert_source_contains(driver, "patchbay-run-60").await?;
        find(driver, By::Css("section.item-labels")).await?;
        assert_state_label_dropdown_and_move(driver).await?;
        send_keys(
            driver,
            By::Css(".label-add-form input[name='key']"),
            "severity",
        )
        .await?;
        send_keys(
            driver,
            By::Css(".label-add-form input[name='value']"),
            "high",
        )
        .await?;
        submit_label_add_form(driver).await?;
        find(driver, By::XPath("//*[contains(text(), 'severity=high')]")).await?;
        assert_label_add_save_preserved_item_page(driver).await?;

        driver
            .goto(app.url("/projects?project=demo"))
            .await
            .context("failed to reopen Patchbay projects page")?;
        find(
            driver,
            By::Css("[data-crudkit-leptos='projects'] .crud-nav"),
        )
        .await?;
        assert_source_contains(driver, "Demo").await?;
        assert_source_does_not_contain(driver, "project-edit-form").await?;

        Ok(())
    }
}

async fn create_project(driver: &WebDriver) -> Result<(), Report> {
    let created = driver
        .execute_async(
            r#"
            const done = arguments[0];
            fetch('/projects', {
                method: 'POST',
                headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
                body: new URLSearchParams({
                    name: 'demo',
                    display_name: 'Demo',
                    path: '.',
                }),
            }).then(response => done(response.ok)).catch(() => done(false));
            "#,
            Vec::new(),
        )
        .await
        .context("failed to create project through browser-test setup request")?
        .convert::<bool>()
        .context("failed to read project setup response")?;
    assert_that!(created).is_true();
    Ok(())
}

async fn create_alternate_project(driver: &WebDriver) -> Result<(), Report> {
    let created = driver
        .execute_async(
            r#"
            const done = arguments[0];
            fetch('/projects', {
                method: 'POST',
                headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
                body: new URLSearchParams({
                    name: 'demo-alt',
                    display_name: 'Demo Alt',
                    path: '.',
                }),
            }).then(response => done(response.ok)).catch(() => done(false));
            "#,
            Vec::new(),
        )
        .await
        .context("failed to create alternate project through browser-test setup request")?
        .convert::<bool>()
        .context("failed to read alternate project setup response")?;
    assert_that!(created).is_true();
    Ok(())
}

async fn create_relationship_target_item(driver: &WebDriver) -> Result<i64, Report> {
    let result = driver
        .execute_async(
            r#"
            const done = arguments[0];
            fetch('/api/projects/demo/items', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({
                    title: 'Relationship target',
                    description: 'Created as a browser-test relationship target',
                    state: 'open',
                    agent_model_override: null,
                    agent_reasoning_effort_override: null,
                }),
            }).then(async response => {
                const body = await response.json();
                done(`${response.status}|${body.id ?? body.error ?? '<missing>'}`);
            }).catch(error => done(`error|${error}`));
            "#,
            Vec::new(),
        )
        .await
        .context("failed to create relationship target through browser-test API request")?
        .convert::<String>()
        .context("failed to read relationship target setup response")?;
    let Some((status, value)) = result.split_once('|') else {
        bail!("unexpected relationship target API response {result:?}");
    };
    assert_that!(status).is_equal_to("200");
    Ok(value
        .parse::<i64>()
        .context("failed to parse relationship target item id")?)
}

async fn seed_memory_history(driver: &WebDriver) -> Result<(), Report> {
    let seeded = driver
        .execute_async(
            r#"
            const done = arguments[0];
            async function setMemory(body) {
                return await fetch('/projects/demo/memory', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
                    body: new URLSearchParams({ body }),
                });
            }
            (async () => {
                const first = await setMemory('Initial shared memory');
                const second = await setMemory('Current shared memory');
                done(first.ok && second.ok ? 'ok' : `failed: ${first.status} ${second.status}`);
            })().catch(error => done(String(error)));
            "#,
            Vec::new(),
        )
        .await
        .context("failed to seed project memory through browser-test setup request")?
        .convert::<String>()
        .context("failed to read memory seed response")?;
    assert_that!(seeded).is_equal_to("ok".to_owned());
    Ok(())
}

async fn seed_system_prompt_history(driver: &WebDriver) -> Result<(), Report> {
    let seeded = driver
        .execute_async(
            r#"
            const done = arguments[0];
            async function setSystemPrompt(body) {
                return await fetch('/projects/demo/system-prompt', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
                    body: new URLSearchParams({ body }),
                });
            }
            (async () => {
                const first = await setSystemPrompt('Initial project prompt');
                const second = await setSystemPrompt('Current project prompt');
                done(first.ok && second.ok ? 'ok' : `failed: ${first.status} ${second.status}`);
            })().catch(error => done(String(error)));
            "#,
            Vec::new(),
        )
        .await
        .context("failed to seed project system prompt through browser-test setup request")?
        .convert::<String>()
        .context("failed to read system prompt seed response")?;
    assert_that!(seeded).is_equal_to("ok".to_owned());
    Ok(())
}

async fn add_agent_comment(driver: &WebDriver) -> Result<(), Report> {
    let created = driver
        .execute_async(
            r#"
            const done = arguments[0];
            const itemId = window.location.pathname.match(/\/items\/(\d+)$/)?.[1];
            if (!itemId) {
                done('missing item id');
                return;
            }
            fetch(`/api/projects/demo/items/${itemId}/comments`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({
                    author_type: 'agent',
                    author_name: 'patchbay-run-60',
                    body: 'Agent progress from browser test',
                }),
            }).then(async response => {
                done(response.ok ? 'ok' : await response.text());
            }).catch(error => done(String(error)));
            "#,
            Vec::new(),
        )
        .await
        .context("failed to add agent comment through API browser-test request")?
        .convert::<String>()
        .context("failed to read agent comment setup response")?;
    assert_that!(created).is_equal_to("ok".to_owned());
    Ok(())
}

async fn claim_current_item(driver: &WebDriver) -> Result<(), Report> {
    let claimed = driver
        .execute_async(
            r#"
            const done = arguments[0];
            const itemId = window.location.pathname.match(/\/items\/(\d+)$/)?.[1];
            if (!itemId) {
                done('missing item id');
                return;
            }
            (async () => {
                const state = 'browser-claimable';
                const moveResponse = await fetch(`/api/projects/demo/items/${itemId}`, {
                    method: 'PATCH',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ state }),
                });
                if (!moveResponse.ok) {
                    done(await moveResponse.text());
                    return;
                }

                const claimResponse = await fetch('/api/projects/demo/items/claim', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({
                        agent_id: 'patchbay-run-60',
                        state,
                    }),
                });
                if (!claimResponse.ok) {
                    done(await claimResponse.text());
                    return;
                }
                const payload = await claimResponse.json();
                if (!payload.item || String(payload.item.id) !== itemId) {
                    done(`claimed wrong item: ${payload.item?.id ?? 'none'}`);
                    return;
                }
                done('ok');
            })().catch(error => done(String(error)));
            "#,
            Vec::new(),
        )
        .await
        .context("failed to claim current item through API browser-test request")?
        .convert::<String>()
        .context("failed to read item claim setup response")?;
    assert_that!(claimed).is_equal_to("ok".to_owned());
    Ok(())
}

async fn assert_memory_history_selector_behaviour(driver: &WebDriver) -> Result<(), Report> {
    let mut ready = false;
    for _ in 0..20 {
        let status = driver
            .execute_async(
                r#"
                const done = arguments[0];
                const textarea = document.querySelector('textarea.project-memory-text');
                if (!textarea) {
                    done('missing textarea');
                    return;
                }
                textarea.value = 'Unsaved current memory';
                textarea.dispatchEvent(new Event('input', { bubbles: true }));
                setTimeout(() => {
                    done(textarea.classList.contains('dirty') ? 'ready' : 'waiting');
                }, 100);
                "#,
                Vec::new(),
            )
            .await
            .context("failed to probe memory history hydration state")?
            .convert::<String>()
            .context("failed to read memory history hydration status")?;

        if status == "ready" {
            ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    if !ready {
        bail!("memory history selector did not become interactive");
    }

    let result = driver
        .execute_async(
            r#"
            const done = arguments[0];
            const select = document.querySelector('#project-memory-version');
            const textarea = document.querySelector('textarea.project-memory-text');
            const save = document.querySelector("form[action='/projects/demo/memory'] button");
            if (!select || !textarea || !save) {
                done('missing memory controls');
                return;
            }
            if (select.value !== 'current') {
                done(`expected current selection, got ${select.value}`);
                return;
            }
            if (select.options.length < 3) {
                done(`expected current plus history options, got ${select.options.length}`);
                return;
            }
            if (textarea.value !== 'Unsaved current memory') {
                done(`expected cached draft before switch, got ${textarea.value}`);
                return;
            }

            select.value = select.options[2].value;
            select.dispatchEvent(new Event('change', { bubbles: true }));
            setTimeout(() => {
                if (textarea.value !== 'Initial shared memory') {
                    done(`expected historical memory, got ${textarea.value}`);
                    return;
                }
                if (!textarea.readOnly) {
                    done('historical memory textarea was editable');
                    return;
                }
                if (textarea.classList.contains('dirty')) {
                    done('historical memory textarea was highlighted');
                    return;
                }
                if (!save.disabled) {
                    done('save button was enabled for historical memory');
                    return;
                }

                select.value = 'current';
                select.dispatchEvent(new Event('change', { bubbles: true }));
                setTimeout(() => {
                    if (textarea.value !== 'Unsaved current memory') {
                        done(`expected cached current draft, got ${textarea.value}`);
                        return;
                    }
                    if (textarea.readOnly) {
                        done('current memory textarea stayed read-only');
                        return;
                    }
                    if (!textarea.classList.contains('dirty')) {
                        done('current memory draft was not highlighted');
                        return;
                    }
                    if (save.disabled) {
                        done('save button stayed disabled for current memory');
                        return;
                    }
                    done('ok');
                }, 100);
            }, 100);
            "#,
            Vec::new(),
        )
        .await
        .context("failed to verify memory history selector behaviour")?
        .convert::<String>()
        .context("failed to read memory history selector result")?;
    assert_that!(result).is_equal_to("ok".to_owned());
    Ok(())
}

async fn assert_system_prompt_history_selector_behaviour(driver: &WebDriver) -> Result<(), Report> {
    let mut ready = false;
    for _ in 0..20 {
        let status = driver
            .execute_async(
                r#"
                const done = arguments[0];
                const textarea = document.querySelector('textarea.project-system-prompt-text');
                if (!textarea) {
                    done('missing textarea');
                    return;
                }
                textarea.value = 'Unsaved current prompt';
                textarea.dispatchEvent(new Event('input', { bubbles: true }));
                setTimeout(() => {
                    done(textarea.classList.contains('dirty') ? 'ready' : 'waiting');
                }, 100);
                "#,
                Vec::new(),
            )
            .await
            .context("failed to probe system prompt history hydration state")?
            .convert::<String>()
            .context("failed to read system prompt history hydration status")?;

        if status == "ready" {
            ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    if !ready {
        bail!("system prompt history selector did not become interactive");
    }

    let result = driver
        .execute_async(
            r#"
            const done = arguments[0];
            const select = document.querySelector('#project-system-prompt-version');
            const textarea = document.querySelector('textarea.project-system-prompt-text');
            const save = document.querySelector("form[action='/projects/demo/system-prompt'] button");
            if (!select || !textarea || !save) {
                done('missing system prompt controls');
                return;
            }
            if (select.value !== 'current') {
                done(`expected current selection, got ${select.value}`);
                return;
            }
            if (select.options.length < 3) {
                done(`expected current plus history options, got ${select.options.length}`);
                return;
            }
            if (textarea.value !== 'Unsaved current prompt') {
                done(`expected cached draft before switch, got ${textarea.value}`);
                return;
            }

            select.value = select.options[2].value;
            select.dispatchEvent(new Event('change', { bubbles: true }));
            setTimeout(() => {
                if (textarea.value !== 'Initial project prompt') {
                    done(`expected historical prompt, got ${textarea.value}`);
                    return;
                }
                if (!textarea.readOnly) {
                    done('historical system prompt textarea was editable');
                    return;
                }
                if (textarea.classList.contains('dirty')) {
                    done('historical system prompt textarea was highlighted');
                    return;
                }
                if (!save.disabled) {
                    done('save button was enabled for historical system prompt');
                    return;
                }

                select.value = 'current';
                select.dispatchEvent(new Event('change', { bubbles: true }));
                setTimeout(() => {
                    if (textarea.value !== 'Unsaved current prompt') {
                        done(`expected cached current draft, got ${textarea.value}`);
                        return;
                    }
                    if (textarea.readOnly) {
                        done('current system prompt textarea stayed read-only');
                        return;
                    }
                    if (!textarea.classList.contains('dirty')) {
                        done('current system prompt draft was not highlighted');
                        return;
                    }
                    if (save.disabled) {
                        done('save button stayed disabled for current system prompt');
                        return;
                    }
                    done('ok');
                }, 100);
            }, 100);
            "#,
            Vec::new(),
        )
        .await
        .context("failed to verify system prompt history selector behaviour")?
        .convert::<String>()
        .context("failed to read system prompt history selector result")?;
    assert_that!(result).is_equal_to("ok".to_owned());
    Ok(())
}

async fn assert_cached_frontend_route_revisit_avoids_loading(
    driver: &WebDriver,
) -> Result<(), Report> {
    click(driver, By::Css(".top-nav a[href='/projects?project=demo']")).await?;
    find(
        driver,
        By::Css("[data-crudkit-leptos='projects'] .crud-nav"),
    )
    .await?;
    click(driver, By::Css(".top-nav a[href='/?project=demo']")).await?;
    find(driver, By::Css("section.board")).await?;

    let result = driver
        .execute_async(
            r#"
            const done = arguments[0];
            const link = document.querySelector(".top-nav a[href='/projects?project=demo']");
            if (!link) {
                done('missing projects link');
                return;
            }
            const isLoadingFallback = () => {
                const fallback = document.querySelector('main.page-shell > p.muted');
                return (fallback?.textContent ?? '').trim() === 'Loading...';
            };
            let sawLoading = isLoadingFallback();
            const observer = new MutationObserver(() => {
                if (isLoadingFallback()) {
                    sawLoading = true;
                }
            });
            observer.observe(document.body, {
                childList: true,
                characterData: true,
                subtree: true,
            });
            link.click();

            const deadline = Date.now() + 5000;
            const check = () => {
                if (document.querySelector("[data-crudkit-leptos='projects'] .crud-nav")) {
                    observer.disconnect();
                    done(`loading=${sawLoading}`);
                    return;
                }
                if (Date.now() > deadline) {
                    observer.disconnect();
                    done(`timeout;loading=${sawLoading};url=${window.location.href}`);
                    return;
                }
                setTimeout(check, 0);
            };
            check();
            "#,
            Vec::new(),
        )
        .await
        .context("failed to verify cached frontend route navigation")?
        .convert::<String>()
        .context("failed to read cached route navigation result")?;
    assert_that!(result).is_equal_to("loading=false".to_owned());

    click(driver, By::Css(".top-nav a[href='/?project=demo']")).await?;
    find(driver, By::Css("section.board")).await?;
    Ok(())
}

async fn assert_crudkit_create_form_survives_live_event(driver: &WebDriver) -> Result<(), Report> {
    click(
        driver,
        By::Css("[data-crudkit-leptos='work-items'] .crud-nav button"),
    )
    .await?;
    find(
        driver,
        By::Css("[data-crudkit-leptos='work-items'] .crud-input-field"),
    )
    .await?;

    let result = driver
        .execute_async(
            r#"
            const done = arguments[0];
            const editableField = () => {
                const panel = document.querySelector("[data-crudkit-leptos='work-items']");
                const field = panel?.querySelector(".crud-input-field");
                if (!field) {
                    return null;
                }
                if (field.matches('input, textarea')) {
                    return field;
                }
                return field.querySelector('input, textarea');
            };
            const draftInput = editableField();
            if (!draftInput) {
                done('missing work-item create input');
                return;
            }
            draftInput.value = 'Draft survives live event';
            draftInput.dispatchEvent(new Event('keyup', { bubbles: true }));
            draftInput.dispatchEvent(new Event('change', { bubbles: true }));

            fetch('/api/projects/demo/items', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({
                    title: 'Live refresh item',
                    description: 'Created to emit a websocket event',
                    state: 'open',
                    agent_model_override: null,
                    agent_reasoning_effort_override: null
                }),
            }).then(async response => {
                if (!response.ok) {
                    done(await response.text());
                    return;
                }
                const deadline = Date.now() + 5000;
                const check = () => {
                    const currentInput = editableField();
                    const boardUpdated = document.body.textContent.includes('Live refresh item');
                    if (currentInput?.value === 'Draft survives live event' && boardUpdated) {
                        done('ok');
                        return;
                    }
                    if (Date.now() > deadline) {
                        done(`draft=${currentInput?.value ?? '<missing>'}; boardUpdated=${boardUpdated}`);
                        return;
                    }
                    setTimeout(check, 100);
                };
                check();
            }).catch(error => done(String(error)));
            "#,
            Vec::new(),
        )
        .await
        .context("failed to verify CrudKit create form survives live event")?
        .convert::<String>()
        .context("failed to read CrudKit live event result")?;
    assert_that!(result).is_equal_to("ok".to_owned());
    Ok(())
}

async fn assert_request_error_toast_preserves_draft(driver: &WebDriver) -> Result<(), Report> {
    let prepared = driver
        .execute_async(
            r#"
            const done = arguments[0];
            const expectedDraft = 'Draft survives request failure';
            const editableField = () => {
                const panel = document.querySelector("[data-crudkit-leptos='work-items']");
                const field = panel?.querySelector(".crud-input-field");
                if (!field) {
                    return null;
                }
                if (field.matches('input, textarea')) {
                    return field;
                }
                return field.querySelector('input, textarea');
            };
            const draftInput = editableField();
            if (!draftInput) {
                done('missing work-item create input');
                return;
            }
            draftInput.value = expectedDraft;
            draftInput.dispatchEvent(new Event('input', { bubbles: true }));
            draftInput.dispatchEvent(new Event('keyup', { bubbles: true }));
            draftInput.dispatchEvent(new Event('change', { bubbles: true }));

            const originalFetch = window.__patchbayOriginalFetch ?? window.fetch.bind(window);
            window.__patchbayOriginalFetch = originalFetch;
            window.__patchbayFailedFetches = [];
            window.__patchbayFailBoardPageRequest = true;
            window.fetch = (input, init) => {
                const rawUrl = typeof input === 'string' ? input : input?.url ?? String(input);
                const url = new URL(rawUrl, window.location.href);
                if (
                    window.__patchbayFailBoardPageRequest &&
                    url.pathname.startsWith('/leptos/load_board_page')
                ) {
                    window.__patchbayFailBoardPageRequest = false;
                    window.__patchbayFailedFetches.push(url.href);
                    return Promise.resolve(new Response('browser-test injected request failure', {
                        status: 503,
                        statusText: 'Browser Test Failure',
                        headers: { 'content-type': 'text/plain' },
                    }));
                }
                return originalFetch(input, init);
            };
            done('ok');
            "#,
            Vec::new(),
        )
        .await
        .context("failed to prepare request-failure browser-test state")?
        .convert::<String>()
        .context("failed to read request-failure preparation result")?;
    assert_that!(prepared).is_equal_to("ok".to_owned());

    click(
        driver,
        By::Css(".project-switcher leptonic-select-selected"),
    )
    .await?;
    click(
        driver,
        By::XPath(
            "//div[contains(@class, 'project-switcher')]//leptonic-select-option[contains(., 'Demo Alt')]",
        ),
    )
    .await?;

    let result = driver
        .execute_async(
            r#"
            const done = arguments[0];
            const expectedDraft = 'Draft survives request failure';
            const deadline = Date.now() + 5000;
            const editableField = () => {
                const panel = document.querySelector("[data-crudkit-leptos='work-items']");
                const field = panel?.querySelector(".crud-input-field");
                if (!field) {
                    return null;
                }
                if (field.matches('input, textarea')) {
                    return field;
                }
                return field.querySelector('input, textarea');
            };
            const restoreFetch = () => {
                if (window.__patchbayOriginalFetch) {
                    window.fetch = window.__patchbayOriginalFetch;
                }
                window.__patchbayFailBoardPageRequest = false;
            };
            const check = () => {
                const failedFetches = window.__patchbayFailedFetches ?? [];
                const draftInput = editableField();
                const toast = document.querySelector('leptonic-toast[data-variant="error"]');
                const toastText = toast?.textContent ?? '';
                const errorPage = document.querySelector('main.error');
                const board = document.querySelector('section.board');
                if (
                    failedFetches.length === 1 &&
                    toastText.includes('Request failed') &&
                    !errorPage &&
                    board &&
                    draftInput?.value === expectedDraft
                ) {
                    restoreFetch();
                    done('ok');
                    return;
                }
                if (Date.now() > deadline) {
                    const report = [
                        `failedFetches=${failedFetches.join(',')}`,
                        `toast=${toastText}`,
                        `errorPage=${Boolean(errorPage)}`,
                        `board=${Boolean(board)}`,
                        `draft=${draftInput?.value ?? '<missing>'}`,
                        `url=${window.location.href}`,
                    ].join('; ');
                    restoreFetch();
                    done(report);
                    return;
                }
                setTimeout(check, 100);
            };
            check();
            "#,
            Vec::new(),
        )
        .await
        .context("failed to verify request error toast behaviour")?
        .convert::<String>()
        .context("failed to read request error toast result")?;
    assert_that!(result).is_equal_to("ok".to_owned());
    driver
        .action_chain()
        .send_keys(Key::Escape)
        .perform()
        .await
        .context("failed to dismiss project switcher after request-failure assertion")?;
    wait_for_no_modal_backdrop_blocking(driver, "after request-failure assertion").await?;

    Ok(())
}

async fn assert_auto_commit_toggle_updates_without_navigation(
    driver: &WebDriver,
) -> Result<(), Report> {
    let initial = driver
        .execute(
            r#"
            const button = document.querySelector('.topbar-auto-commit[role="switch"]');
            const checkbox = document.querySelector('#project-auto-commit');
            window.__patchbayAutoCommitMarker = 'alive';
            window.__patchbayAutoCommitUrl = window.location.href;
            return `${button?.getAttribute('aria-checked') ?? 'missing'}|${checkbox?.checked ?? 'missing'}`;
            "#,
            Vec::new(),
        )
        .await
        .context("failed to inspect initial Auto-Commit state")?
        .convert::<String>()
        .context("failed to read initial Auto-Commit state")?;
    assert_that!(initial).is_equal_to("true|true".to_owned());

    click(driver, By::Css(".topbar-auto-commit[role='switch']")).await?;

    let result = driver
        .execute_async(
            r#"
            const done = arguments[0];
            const deadline = Date.now() + 5000;
            async function check() {
                const button = document.querySelector('.topbar-auto-commit[role="switch"]');
                const checkbox = document.querySelector('#project-auto-commit');
                const marker = window.__patchbayAutoCommitMarker;
                const sameUrl = window.location.href === window.__patchbayAutoCommitUrl;
                const checked = button?.getAttribute('aria-checked') ?? 'missing';
                const settingsChecked = checkbox?.checked ?? 'missing';
                let persisted = 'not checked';
                try {
                    const response = await fetch('/api/projects/demo/settings');
                    persisted = response.ok ? (await response.json()).auto_commit : `status ${response.status}`;
                } catch (error) {
                    persisted = String(error);
                }

                if (
                    marker === 'alive' &&
                    sameUrl &&
                    checked === 'false' &&
                    settingsChecked === false &&
                    persisted === false
                ) {
                    done('ok');
                    return;
                }
                if (Date.now() > deadline) {
                    done(`marker=${marker}; sameUrl=${sameUrl}; checked=${checked}; settingsChecked=${settingsChecked}; persisted=${persisted}`);
                    return;
                }
                setTimeout(check, 100);
            }
            check();
            "#,
            Vec::new(),
        )
        .await
        .context("failed to verify Auto-Commit toggle behaviour")?
        .convert::<String>()
        .context("failed to read Auto-Commit toggle result")?;
    assert_that!(result).is_equal_to("ok".to_owned());

    Ok(())
}

async fn assert_settings_response_omits_refinement_policy(
    driver: &WebDriver,
) -> Result<(), Report> {
    let result = driver
        .execute_async(
            r#"
            const done = arguments[0];
            fetch('/api/projects/demo/settings')
                .then(async (response) => {
                    if (!response.ok) {
                        done(`status ${response.status}`);
                        return;
                    }
                    const settings = await response.json();
                    const legacyKey = ['allow', 'refinement', 'agents', 'during', 'editing'].join('_');
                    done(Object.hasOwn(settings, legacyKey) ? 'present' : 'absent');
                })
                .catch((error) => done(String(error)));
            "#,
            Vec::new(),
        )
        .await
        .context("failed to inspect project settings API response")?
        .convert::<String>()
        .context("failed to read project settings API field check")?;
    assert_that!(result).is_equal_to("absent".to_owned());
    Ok(())
}

async fn create_trigger(driver: &WebDriver) -> Result<(), Report> {
    let created = driver
        .execute_async(
            r#"
            const done = arguments[0];
            fetch('/api/automation_triggers/crud/create-one', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({
                    entity: {
                        project_id: 1,
                        name: 'refine-new',
                        enabled: true,
                        activation: 'work_item_created',
                        effect: 'consume_work',
                        schedule: '@every 15s',
                        tool_name: 'codex',
                        mutability: 'read_only',
                        prompt: 'Refine new work items.',
                        work_item_selector: '{"All":[{"column_name":"state","operator":"=","value":{"String":"open"}}]}',
                        priority: 0
                    }
                }),
            }).then(async response => {
                done(response.ok ? 'ok' : await response.text());
            }).catch(error => done(String(error)));
            "#,
            Vec::new(),
        )
        .await
        .context("failed to create trigger through CrudKit browser-test setup request")?
        .convert::<String>()
        .context("failed to read trigger setup response")?;
    assert_that!(created).is_equal_to("ok".to_owned());
    Ok(())
}

async fn open_new_item_modal(driver: &WebDriver) -> Result<(), Report> {
    let mut last_state = inspect_new_item_modal_state(driver).await?;
    for _ in 0..20 {
        if last_state.starts_with("modalVisible=true;") && last_state.contains("formReady=true") {
            return Ok(());
        }
        if !last_state.starts_with("modalVisible=true;") {
            click_css_after_modal_backdrops_clear(
                driver,
                "section.board-toolbar > button",
                "opening new item modal",
            )
            .await?;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
        last_state = inspect_new_item_modal_state(driver).await?;
    }
    bail!("new item modal did not open: {last_state}");
}

async fn assert_lane_add_button_count(driver: &WebDriver, expected: usize) -> Result<(), Report> {
    let count = driver
        .execute(
            "return String(document.querySelectorAll('.lane .lane-add').length);",
            Vec::new(),
        )
        .await
        .context("failed to count lane add buttons")?
        .convert::<String>()
        .context("failed to read lane add button count")?;
    assert_that!(count).is_equal_to(expected.to_string());
    Ok(())
}

async fn assert_board_shell_uses_viewport_width(driver: &WebDriver) -> Result<(), Report> {
    driver
        .set_window_rect(0, 0, 1800, 1000)
        .await
        .context("failed to widen browser test window")?;
    let summary = driver
        .execute(
            r#"
            const shell = document.querySelector('main.page-shell');
            if (!shell) {
                throw new Error('missing page shell');
            }
            const shellWidth = Math.round(shell.getBoundingClientRect().width);
            const viewportWidth = document.documentElement.clientWidth;
            return `${shellWidth}|${viewportWidth}`;
            "#,
            Vec::new(),
        )
        .await
        .context("failed to inspect board shell width")?
        .convert::<String>()
        .context("failed to read board shell width")?;
    let Some((shell_width, viewport_width)) = summary.split_once('|') else {
        bail!("failed to parse board shell width summary {summary:?}");
    };
    let shell_width = shell_width
        .parse::<i64>()
        .context("failed to parse shell width")?;
    let viewport_width = viewport_width
        .parse::<i64>()
        .context("failed to parse viewport width")?;
    if shell_width < viewport_width - 1 {
        bail!("board shell width {shell_width}px did not fill viewport width {viewport_width}px");
    }
    Ok(())
}

async fn assert_new_item_lane_options(driver: &WebDriver) -> Result<(), Report> {
    let summary = driver
        .execute(
            r#"
            const select = document.querySelector('#new-item-modal select[name="state"]');
            if (!select) {
                throw new Error('missing new item state select');
            }
            return `${select.value}|${Array.from(select.options).map(option => option.value).join(',')}`;
            "#,
            Vec::new(),
        )
        .await
        .context("failed to inspect new item state options")?
        .convert::<String>()
        .context("failed to read new item state options")?;
    assert_that!(summary).is_equal_to("idea|idea,open,in_progress,done".to_owned());
    Ok(())
}

async fn assert_new_item_modal_actions(driver: &WebDriver) -> Result<(), Report> {
    let summary = driver
        .execute(
            r#"
            const modal = document.querySelector('#new-item-modal');
            if (!modal) {
                throw new Error('missing new item modal');
            }
            const headerButton = modal.querySelector('leptonic-modal-header button');
            const bodySaveButton = modal.querySelector('leptonic-modal-body .crud-nav button');
            const footerButtons = Array.from(modal.querySelectorAll('leptonic-modal-footer button'));
            const footerButtonTexts = footerButtons
                .map(button => (button.textContent ?? '').replace(/\s+/g, ' ').trim())
                .join('|');
            return [
                `headerIcon=${Boolean(headerButton?.querySelector('svg'))}`,
                `headerText=${(headerButton?.textContent ?? '').replace(/\s+/g, ' ').trim() || '<empty>'}`,
                `bodySave=${Boolean(bodySaveButton)}`,
                `footerButtons=${footerButtonTexts}`,
            ].join(';');
            "#,
            Vec::new(),
        )
        .await
        .context("failed to inspect new item modal actions")?
        .convert::<String>()
        .context("failed to read new item modal action summary")?;
    assert_that!(summary).is_equal_to(
        "headerIcon=true;headerText=<empty>;bodySave=false;footerButtons=Cancel|Speichern"
            .to_owned(),
    );
    Ok(())
}

async fn assert_lane_add_preselects_state(driver: &WebDriver) -> Result<(), Report> {
    click_css_after_modal_backdrops_clear(
        driver,
        ".lane:nth-child(2) .lane-add",
        "opening lane-preselected new item modal",
    )
    .await?;
    find(driver, By::Css("#new-item-modal select[name='state']")).await?;
    let summary = driver
        .execute(
            r#"
            const select = document.querySelector('#new-item-modal select[name="state"]');
            if (!select) {
                throw new Error('missing new item state select');
            }
            return `${select.value}|${Array.from(select.options).map(option => option.value).join(',')}`;
            "#,
            Vec::new(),
        )
        .await
        .context("failed to inspect lane-preselected new item state")?
        .convert::<String>()
        .context("failed to read lane-preselected new item state")?;
    assert_that!(summary).is_equal_to("open|open".to_owned());
    close_clean_new_item_modal(driver).await?;
    Ok(())
}

async fn assert_new_item_modal_dirty_leave_protection(driver: &WebDriver) -> Result<(), Report> {
    open_new_item_modal(driver).await?;
    set_input_value(
        driver,
        "#new-item-modal .crud-input-field",
        "Unsaved modal title",
    )
    .await?;

    click(
        driver,
        By::Css("#new-item-modal leptonic-modal-header button"),
    )
    .await?;
    find_leave_modal(driver).await?;
    click_leave_modal_cancel(driver).await?;
    assert_new_item_title_value(driver, "Unsaved modal title").await?;

    driver
        .action_chain()
        .send_keys(Key::Escape)
        .perform()
        .await
        .context("failed to press Escape for new item modal")?;
    find_leave_modal(driver).await?;
    click_leave_modal_cancel(driver).await?;
    assert_new_item_title_value(driver, "Unsaved modal title").await?;

    click_backdrop(driver).await?;
    find_leave_modal(driver).await?;
    click_leave_modal_accept(driver).await?;
    wait_for_new_item_modal_closed(driver).await?;

    open_new_item_modal(driver).await?;
    append_new_item_initial_label(driver, "area", "unsaved").await?;
    click(
        driver,
        By::Css("#new-item-modal leptonic-modal-header button"),
    )
    .await?;
    find_leave_modal(driver).await?;
    click_leave_modal_cancel(driver).await?;
    assert_new_item_initial_label_value(driver, "area", "unsaved").await?;

    click_backdrop(driver).await?;
    find_leave_modal(driver).await?;
    click_leave_modal_accept(driver).await?;
    wait_for_new_item_modal_closed(driver).await?;
    Ok(())
}

async fn assert_item_detail_dirty_leave_protection(driver: &WebDriver) -> Result<(), Report> {
    set_input_value(
        driver,
        "section.item-settings .crud-input-field",
        "Unsaved detail title",
    )
    .await?;

    click(driver, By::Css("button.item-board-link")).await?;
    find_leave_modal(driver).await?;
    click_leave_modal_cancel(driver).await?;
    assert_source_contains(driver, "Unsaved detail title").await?;

    click(driver, By::Css("button.item-board-link")).await?;
    find_leave_modal(driver).await?;
    click_leave_modal_accept(driver).await?;
    find(driver, By::Css("section.board")).await?;
    Ok(())
}

async fn close_clean_new_item_modal(driver: &WebDriver) -> Result<(), Report> {
    click(
        driver,
        By::Css("#new-item-modal leptonic-modal-footer button"),
    )
    .await?;
    wait_for_new_item_modal_closed(driver).await
}

async fn click_new_item_save(driver: &WebDriver) -> Result<(), Report> {
    click(
        driver,
        By::XPath("//leptonic-modal[@id='new-item-modal']//button[contains(., 'Speichern')]"),
    )
    .await
}

async fn find_leave_modal(driver: &WebDriver) -> Result<(), Report> {
    find(
        driver,
        By::XPath("//leptonic-modal[contains(., 'Ungespeicherte Änderungen')]"),
    )
    .await
    .map(|_| ())
}

async fn click_leave_modal_cancel(driver: &WebDriver) -> Result<(), Report> {
    click(
        driver,
        By::XPath(
            "//leptonic-modal[contains(., 'Ungespeicherte Änderungen')]//button[contains(., 'Zurück')]",
        ),
    )
    .await
}

async fn click_leave_modal_accept(driver: &WebDriver) -> Result<(), Report> {
    click(
        driver,
        By::XPath(
            "//leptonic-modal[contains(., 'Ungespeicherte Änderungen')]//button[contains(., 'Verlassen')]",
        ),
    )
    .await
}

async fn assert_new_item_title_value(driver: &WebDriver, expected: &str) -> Result<(), Report> {
    let value = driver
        .execute(
            r#"
            const editable = (field) => {
                if (!field) {
                    return null;
                }
                if (field.matches('input, textarea, select')) {
                    return field;
                }
                return field.querySelector('input, textarea, select');
            };
            const field = document.querySelector('#new-item-modal .crud-input-field');
            const input = editable(field);
            return input?.value ?? '<missing>';
            "#,
            Vec::new(),
        )
        .await
        .context("failed to inspect new item title draft")?
        .convert::<String>()
        .context("failed to read new item title draft")?;
    assert_that!(value).is_equal_to(expected.to_owned());
    Ok(())
}

async fn append_new_item_initial_label(
    driver: &WebDriver,
    key: &str,
    value: &str,
) -> Result<(), Report> {
    let script = format!(
        r#"
        const done = arguments[0];
        const key = {key:?};
        const value = {value:?};
        const add = document.querySelector('#new-item-modal .initial-label-add');
        if (!add) {{
            done('missing add button');
            return;
        }}
        const before = document.querySelectorAll('#new-item-modal .initial-label-row').length;
        add.click();
        const deadline = Date.now() + 5000;
        const setValue = (input, next) => {{
            input.value = next;
            input.setAttribute('value', next);
            input.dispatchEvent(new Event('input', {{ bubbles: true }}));
            input.dispatchEvent(new Event('change', {{ bubbles: true }}));
        }};
        const fill = () => {{
            const rows = document.querySelectorAll('#new-item-modal .initial-label-row');
            if (rows.length <= before) {{
                if (Date.now() > deadline) {{
                    done(`row count stayed at ${{before}}`);
                    return;
                }}
                setTimeout(fill, 100);
                return;
            }}
            const row = rows[rows.length - 1];
            const keyInput = row.querySelector('.initial-label-key');
            const valueInput = row.querySelector('.initial-label-value');
            if (!keyInput || !valueInput) {{
                done('missing row inputs');
                return;
            }}
            setValue(keyInput, key);
            setValue(valueInput, value);
            done('ok');
        }};
        fill();
        "#
    );
    let result = driver
        .execute_async(script, Vec::new())
        .await
        .context("failed to append new item initial label")?
        .convert::<String>()
        .context("failed to read initial label append result")?;
    if result != "ok" {
        bail!("failed to append new item initial label: {result}");
    }
    Ok(())
}

async fn assert_new_item_initial_label_value(
    driver: &WebDriver,
    expected_key: &str,
    expected_value: &str,
) -> Result<(), Report> {
    let summary = driver
        .execute(
            r#"
            const row = document.querySelector('#new-item-modal .initial-label-row');
            const key = row?.querySelector('.initial-label-key')?.value ?? '<missing>';
            const value = row?.querySelector('.initial-label-value')?.value ?? '<missing>';
            return `${key}|${value}`;
            "#,
            Vec::new(),
        )
        .await
        .context("failed to inspect initial label draft")?
        .convert::<String>()
        .context("failed to read initial label draft")?;
    assert_that!(summary).is_equal_to(format!("{expected_key}|{expected_value}"));
    Ok(())
}

async fn assert_board_card_contains(
    driver: &WebDriver,
    title: &str,
    expected: &str,
) -> Result<(), Report> {
    let script = format!(
        r#"
        const title = {title:?};
        const expected = {expected:?};
        const link = Array.from(document.querySelectorAll('article.card a'))
            .find((link) => (link.textContent ?? '').includes(title));
        const card = link?.closest('article.card');
        return card ? String((card.textContent ?? '').includes(expected)) : 'missing-card';
        "#
    );
    let result = driver
        .execute(script, Vec::new())
        .await
        .context("failed to inspect board card labels")?
        .convert::<String>()
        .context("failed to read board card label summary")?;
    assert_that!(result).is_equal_to("true".to_owned());
    Ok(())
}

async fn click_backdrop(driver: &WebDriver) -> Result<(), Report> {
    driver
        .action_chain()
        .move_to(4, 4)
        .click()
        .perform()
        .await
        .context("failed to click modal backdrop")?;
    Ok(())
}

async fn wait_for_new_item_modal_closed(driver: &WebDriver) -> Result<(), Report> {
    let result = driver
        .execute_async(
            r#"
            const done = arguments[0];
            const deadline = Date.now() + 5000;
            const isVisible = (element) => {
                if (!element) {
                    return false;
                }
                const style = getComputedStyle(element);
                const rect = element.getBoundingClientRect();
                return style.display !== 'none' &&
                    style.visibility !== 'hidden' &&
                    rect.width > 0 &&
                    rect.height > 0;
            };
            const check = () => {
                const modal = document.querySelector('leptonic-modal#new-item-modal');
                const modalVisible = isVisible(modal);
                const backdropState = Array.from(
                    document.querySelectorAll('leptonic-modal-backdrop')
                ).map((backdrop) => {
                    const style = getComputedStyle(backdrop);
                    return {
                        visible: isVisible(backdrop),
                        blocking: style.pointerEvents !== 'none',
                    };
                });
                const backdropVisible = backdropState.some((state) => state.visible);
                const backdropBlocking = backdropState.some((state) => state.blocking);
                const hit = document.elementFromPoint(
                    Math.max(1, document.documentElement.clientWidth - 68),
                    143
                );
                const hitBackdrop = hit?.tagName === 'LEPTONIC-MODAL-BACKDROP' ||
                    Boolean(hit?.closest?.('leptonic-modal-backdrop'));
                if (!modalVisible && !backdropVisible && !backdropBlocking && !hitBackdrop) {
                    done('closed');
                    return;
                }
                if (Date.now() > deadline) {
                    const host = document.querySelector('leptonic-modal-host');
                    const hostStyle = host ? getComputedStyle(host) : null;
                    const modalSummary = Array.from(document.querySelectorAll('leptonic-modal'))
                        .map((modal) => {
                            const style = getComputedStyle(modal);
                            const text = (modal.textContent ?? '').replace(/\s+/g, ' ').trim().slice(0, 80);
                            return `${modal.id || '<no-id>'}:${style.display}:${style.visibility}:${text}`;
                        })
                        .join(' || ');
                    done([
                        `still-open: modal=${modalVisible}`,
                        `backdrop=${backdropVisible}`,
                        `blocking=${backdropBlocking}`,
                        `hit=${hit?.tagName ?? '<none>'}`,
                        `hostHasModals=${host?.getAttribute('data-has-modals') ?? '<missing>'}`,
                        `hostDisplay=${hostStyle?.display ?? '<missing>'}`,
                        `modals=${modalSummary}`,
                    ].join('; '));
                    return;
                }
                setTimeout(check, 100);
            };
            check();
            "#,
            Vec::new(),
        )
        .await
        .context("failed to wait for new item modal close")?
        .convert::<String>()
        .context("failed to read new item modal close state")?;
    assert_that!(result).is_equal_to("closed".to_owned());
    Ok(())
}

async fn wait_for_no_modal_backdrop_blocking(
    driver: &WebDriver,
    context: &str,
) -> Result<(), Report> {
    let result = driver
        .execute_async(
            r#"
            const done = arguments[0];
            const deadline = Date.now() + 3000;
            const check = () => {
                const hit = document.elementFromPoint(
                    Math.max(1, document.documentElement.clientWidth - 68),
                    143
                );
                const blocking = hit?.tagName === 'LEPTONIC-MODAL-BACKDROP' ||
                    Boolean(hit?.closest?.('leptonic-modal-backdrop'));
                if (!blocking) {
                    done('clear');
                    return;
                }
                if (Date.now() > deadline) {
                    const modalSummary = Array.from(document.querySelectorAll('leptonic-modal'))
                        .map((modal) => {
                            const style = getComputedStyle(modal);
                            const rect = modal.getBoundingClientRect();
                            const text = (modal.textContent ?? '').replace(/\s+/g, ' ').trim().slice(0, 80);
                            return `${modal.id || '<no-id>'}:${style.display}:${style.visibility}:${Math.round(rect.width)}x${Math.round(rect.height)}:${text}`;
                        })
                        .join(' || ');
                    const selectSummary = Array.from(document.querySelectorAll('leptonic-select, leptonic-select-overlay, leptonic-select-options'))
                        .map((select) => {
                            const style = getComputedStyle(select);
                            const rect = select.getBoundingClientRect();
                            const text = (select.textContent ?? '').replace(/\s+/g, ' ').trim().slice(0, 80);
                            return `${select.tagName}:${style.display}:${style.visibility}:${Math.round(rect.width)}x${Math.round(rect.height)}:${text}`;
                        })
                        .join(' || ');
                    done(`blocked-by=${hit?.tagName ?? '<none>'}; modals=${modalSummary}; selects=${selectSummary}`);
                    return;
                }
                setTimeout(check, 100);
            };
            check();
            "#,
            Vec::new(),
        )
        .await
        .context("failed to wait for modal backdrop hit-test")?
        .convert::<String>()
        .context("failed to read modal backdrop hit-test state")?;
    if result != "clear" {
        bail!("modal backdrop still blocking {context}: {result}");
    }
    Ok(())
}

async fn click_css_after_modal_backdrops_clear(
    driver: &WebDriver,
    selector: &str,
    context: &str,
) -> Result<(), Report> {
    let mut last_error = None;
    for _ in 0..20 {
        wait_for_no_modal_backdrop_blocking(driver, context).await?;
        let element = find(driver, By::Css(selector)).await?;
        element
            .scroll_into_view()
            .await
            .context("failed to scroll browser-test element into view")?;
        driver
            .action_chain()
            .move_to_element_center(&element)
            .perform()
            .await
            .context("failed to move pointer to browser-test element")?;
        tokio::time::sleep(Duration::from_millis(150)).await;
        match element.click().await {
            Ok(()) => return Ok(()),
            Err(error) => {
                last_error = Some(error.to_string());
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        }
    }

    bail!(
        "failed to click browser-test element {selector:?} while {context}: {}",
        last_error.unwrap_or_else(|| "no click attempt was made".to_owned())
    );
}

async fn inspect_new_item_modal_state(driver: &WebDriver) -> Result<String, Report> {
    Ok(driver
        .execute(
            r#"
            const modal = document.querySelector('leptonic-modal#new-item-modal');
            const button = document.querySelector('section.board-toolbar > button');
            const host = document.querySelector('leptonic-modal-host');
            const hostStyle = host ? getComputedStyle(host) : null;
            const bodyText = document.body.textContent ?? '';
            const isVisible = (element) => {
                if (!element) {
                    return false;
                }
                const style = getComputedStyle(element);
                const rect = element.getBoundingClientRect();
                return style.display !== 'none' &&
                    style.visibility !== 'hidden' &&
                    rect.width > 0 &&
                    rect.height > 0;
            };
            const editable = (field) => {
                if (!field) {
                    return null;
                }
                if (field.matches('input, textarea, select')) {
                    return field;
                }
                return field.querySelector('input, textarea, select');
            };
            const titleField = modal?.querySelector('.crud-input-field');
            const titleInput = editable(titleField);
            const stateSelect = modal?.querySelector('select[name="state"]');
            return [
                `modalVisible=${isVisible(modal)}`,
                `modal=${Boolean(modal)}`,
                `formReady=${Boolean(titleInput && stateSelect)}`,
                `buttonDisabled=${button?.disabled ?? '<missing>'}`,
                `buttonText=${(button?.textContent ?? '<missing>').trim()}`,
                `hostHasModals=${host?.getAttribute('data-has-modals') ?? '<missing>'}`,
                `hostDisplay=${hostStyle?.display ?? '<missing>'}`,
                `hostContainsModal=${Boolean(host?.querySelector('leptonic-modal'))}`,
                `htmlOverflow=${document.documentElement.style.overflow || '<empty>'}`,
                `lanes=${document.querySelectorAll('.lane').length}`,
                `laneAdds=${document.querySelectorAll('.lane .lane-add').length}`,
                `boardWarning=${bodyText.includes('No work item states') || bodyText.includes('state')}`,
            ].join('; ');
            "#,
            Vec::new(),
        )
        .await
        .context("failed to inspect new item modal state")?
        .convert::<String>()
        .context("failed to read new item modal state")?)
}

async fn assert_item_detail_description_is_not_duplicated(
    driver: &WebDriver,
) -> Result<(), Report> {
    let summary = driver
        .execute_async(
            r#"
            const done = arguments[0];
            const expected = 'Created through browser-test\nSecond line';
            const deadline = Date.now() + 5000;
            const inspect = () => {
                const headerText = document.querySelector('.item-header')?.textContent ?? '';
                const input = document.querySelector(
                    'section.item-settings input[name="description"]'
                );
                const editor = document.querySelector(
                    'section.item-settings [data-rich-text-field="description"] leptonic-tiptap-editor'
                );
                const descriptionValue = input?.value ?? '';
                const result = [
                    headerText.includes(expected),
                    descriptionValue === expected || (
                        descriptionValue.includes('Created through browser-test') &&
                        descriptionValue.includes('Second line')
                    ),
                    Boolean(editor)
                ].join('|');
                if (result === 'false|true|true' || Date.now() > deadline) {
                    done(result);
                    return;
                }
                setTimeout(inspect, 100);
            };
            inspect();
            "#,
            Vec::new(),
        )
        .await
        .context("failed to inspect item detail description placement")?
        .convert::<String>()
        .context("failed to read item detail description placement")?;
    assert_that!(summary).is_equal_to("false|true|true".to_owned());
    Ok(())
}

async fn assert_item_detail_description_editor_accepts_click_and_text(
    driver: &WebDriver,
) -> Result<(), Report> {
    driver
        .execute(
            r#"window.__patchbayDescriptionEditorClickMarker = 'kept';"#,
            Vec::new(),
        )
        .await
        .context("failed to set description editor click marker")?;

    click_description_editor(driver).await?;
    tokio::time::sleep(Duration::from_millis(250)).await;

    let marker = driver
        .execute(
            r#"
            return window.__patchbayDescriptionEditorClickMarker === 'kept'
                ? 'kept'
                : 'lost';
            "#,
            Vec::new(),
        )
        .await
        .context("failed to inspect description editor click marker")?
        .convert::<String>()
        .context("failed to read description editor click marker")?;
    assert_that!(marker).is_equal_to("kept".to_owned());

    let editor = find(
        driver,
        By::Css("section.item-settings [data-rich-text-field='description'] .ProseMirror"),
    )
    .await?;
    editor
        .send_keys(" Editable after click")
        .await
        .context("failed to type in description editor after click")?;

    let value = driver
        .execute_async(
            r#"
            const done = arguments[0];
            const deadline = Date.now() + 5000;
            const inspect = () => {
                const input = document.querySelector(
                    'section.item-settings input[name="description"]'
                );
                const value = input?.value ?? '';
                if (value.includes('Editable after click') || Date.now() > deadline) {
                    done(value);
                    return;
                }
                setTimeout(inspect, 100);
            };
            inspect();
            "#,
            Vec::new(),
        )
        .await
        .context("failed to inspect edited description value")?
        .convert::<String>()
        .context("failed to read edited description value")?;
    assert_that!(value).contains("Editable after click");
    Ok(())
}

async fn click_description_editor(driver: &WebDriver) -> Result<(), Report> {
    let selector = "section.item-settings [data-rich-text-field='description'] .ProseMirror";
    let mut last_error = None;
    for _ in 0..5 {
        let editor = find(driver, By::Css(selector)).await?;
        match editor.scroll_into_view().await {
            Ok(()) => match editor.click().await {
                Ok(()) => return Ok(()),
                Err(err) => last_error = Some(format!("click failed: {err}")),
            },
            Err(err) => last_error = Some(format!("scroll failed: {err}")),
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    bail!(
        "failed to click description editor after retries: {}",
        last_error.unwrap_or_else(|| "no click attempt was made".to_owned())
    )
}

async fn assert_item_relationship_create_delete_flow(
    driver: &WebDriver,
    target_id: i64,
) -> Result<(), Report> {
    find(driver, By::Css("section.item-relationships")).await?;
    assert_source_contains(driver, "No relationships").await?;

    let add_script = r#"
        const targetId = TARGET_ID;
        const form = document.querySelector('.relationship-add-form');
        if (!form) {
            throw new Error('missing relationship add form');
        }
        window.__patchbayRelationshipUrl = window.location.href;
        window.__patchbayRelationshipMarker = 'created';
        document.body.style.minHeight = '5000px';
        window.scrollTo(0, 1800);
        form.querySelector('input[name="target_work_item_id"]').value = String(targetId);
        form.querySelector('input[name="kind"]').value = 'is follow-up of';
        form.requestSubmit();
    "#
    .replace("TARGET_ID", &target_id.to_string());
    driver
        .execute(add_script, Vec::new())
        .await
        .context("failed to submit relationship add form")?;

    let wait_create_script = r#"
        const targetId = TARGET_ID;
        const done = arguments[0];
        const deadline = Date.now() + 5000;
        const inspect = () => {
            const panel = document.querySelector('section.item-relationships');
            const text = panel?.innerText ?? '';
            const related = panel?.querySelector(`.relationship-related[href="/projects/demo/items/${targetId}"]`);
            const summary = [
                `marker=${window.__patchbayRelationshipMarker ?? '<missing>'}`,
                `sameUrl=${window.location.href === window.__patchbayRelationshipUrl}`,
                `scrollKept=${window.scrollY > 1000}`,
                `hasRow=${Boolean(panel?.querySelector('.relationship-row'))}`,
                `hasRelated=${Boolean(related)}`,
                `hasKind=${text.includes('is follow-up of')}`,
                `hasDirection=${text.includes('outgoing')}`,
                `hasTitle=${text.includes('Relationship target')}`,
            ].join(';');
            if (summary.endsWith('hasTitle=true') || Date.now() > deadline) {
                done(summary);
                return;
            }
            setTimeout(inspect, 100);
        };
        inspect();
    "#
    .replace("TARGET_ID", &target_id.to_string());
    let created_summary = driver
        .execute_async(wait_create_script, Vec::new())
        .await
        .context("failed to wait for relationship row after add")?
        .convert::<String>()
        .context("failed to read relationship add summary")?;
    assert_that!(created_summary).is_equal_to(
        "marker=created;sameUrl=true;scrollKept=true;hasRow=true;hasRelated=true;hasKind=true;hasDirection=true;hasTitle=true"
            .to_owned(),
    );

    driver
        .execute(
            r#"
            const form = document.querySelector('.relationship-row form[action$="/delete"]');
            if (!form) {
                throw new Error('missing relationship delete form');
            }
            window.__patchbayRelationshipMarker = 'deleted';
            form.requestSubmit();
            "#,
            Vec::new(),
        )
        .await
        .context("failed to submit relationship delete form")?;

    let deleted_summary = driver
        .execute_async(
            r#"
            const done = arguments[0];
            const deadline = Date.now() + 5000;
            const inspect = () => {
                const panel = document.querySelector('section.item-relationships');
                const text = panel?.innerText ?? '';
                const summary = [
                    `marker=${window.__patchbayRelationshipMarker ?? '<missing>'}`,
                    `sameUrl=${window.location.href === window.__patchbayRelationshipUrl}`,
                    `hasRow=${Boolean(panel?.querySelector('.relationship-row'))}`,
                    `empty=${text.includes('No relationships')}`,
                ].join(';');
                if (summary.endsWith('empty=true') || Date.now() > deadline) {
                    document.body.style.minHeight = '';
                    done(summary);
                    return;
                }
                setTimeout(inspect, 100);
            };
            inspect();
            "#,
            Vec::new(),
        )
        .await
        .context("failed to wait for relationship row after delete")?
        .convert::<String>()
        .context("failed to read relationship delete summary")?;
    assert_that!(deleted_summary)
        .is_equal_to("marker=deleted;sameUrl=true;hasRow=false;empty=true".to_owned());
    Ok(())
}

async fn assert_source_contains(driver: &WebDriver, expected: &str) -> Result<(), Report> {
    let source = driver
        .source()
        .await
        .context("failed to read page source")?;
    assert_that!(source).contains(expected);
    Ok(())
}

async fn assert_top_nav_order(driver: &WebDriver) -> Result<(), Report> {
    let labels = driver
        .execute(
            r#"
            return Array.from(document.querySelectorAll('.top-nav a'))
                .map((link) => link.textContent.trim())
                .join('|');
            "#,
            Vec::new(),
        )
        .await
        .context("failed to inspect top navigation")?
        .convert::<String>()
        .context("failed to read top navigation labels")?;
    assert_that!(labels).is_equal_to("Board|Automation|Runs|Projects|API".to_owned());
    Ok(())
}

async fn assert_codex_auth_guide_when_blocked(driver: &WebDriver) -> Result<(), Report> {
    let source = driver
        .source()
        .await
        .context("failed to read page source")?;
    if source.contains("Codex automation blocked") && source.contains("Not signed in") {
        for expected in [
            "Sign in to Codex",
            "CODEX_HOME=",
            "CODEX_SQLITE_HOME=",
            "Copy command",
            "Copy home",
            "Log out",
            "/codex/logout",
            "OPENAI_API_KEY",
        ] {
            if !source.contains(expected) {
                bail!("blocked Codex auth guide did not include {expected:?}");
            }
        }
        if source.contains("Install Codex and make sure") {
            bail!("blocked Codex auth guide unexpectedly included the install prompt");
        }
    }
    Ok(())
}

async fn assert_source_does_not_contain(
    driver: &WebDriver,
    unexpected: &str,
) -> Result<(), Report> {
    let source = driver
        .source()
        .await
        .context("failed to read page source")?;
    if source.contains(unexpected) {
        bail!("page source unexpectedly contained {unexpected:?}");
    }
    Ok(())
}

async fn find(driver: &WebDriver, by: By) -> Result<browser_test::thirtyfour::WebElement, Report> {
    match driver.find(by).await {
        Ok(element) => Ok(element),
        Err(err) => {
            let current_url = driver
                .current_url()
                .await
                .map(|url| url.to_string())
                .unwrap_or_else(|url_err| format!("failed to read current URL: {url_err}"));
            let source = driver
                .source()
                .await
                .unwrap_or_else(|source_err| format!("failed to read page source: {source_err}"));
            let source_prefix = source.chars().take(4_000).collect::<String>();
            bail!(
                "failed to find browser-test element at {current_url}: {err}; source prefix: {source_prefix}"
            );
        }
    }
}

async fn click(driver: &WebDriver, by: By) -> Result<(), Report> {
    let target = format!("{by:?}");
    let element = find(driver, by).await?;
    if let Err(initial_err) = element.click().await {
        element
            .scroll_into_view()
            .await
            .context("failed to scroll browser-test element into view")?;
        tokio::time::sleep(Duration::from_millis(100)).await;
        if let Err(retry_err) = element.click().await {
            bail!(
                "failed to click browser-test element {target}: {retry_err}; initial error: {initial_err}"
            );
        }
    }
    Ok(())
}

async fn submit_label_add_form(driver: &WebDriver) -> Result<(), Report> {
    driver
        .execute(
            r#"
            const form = document.querySelector('.label-add-form');
            if (!form) {
                throw new Error('missing label add form');
            }
            window.__patchbayLabelAddMarker = 'alive';
            window.__patchbayLabelAddUrl = window.location.href;
            document.body.style.minHeight = '5000px';
            window.scrollTo(0, 1600);
            form.requestSubmit();
            "#,
            Vec::new(),
        )
        .await
        .context("failed to submit label add form")?;
    Ok(())
}

async fn assert_label_add_save_preserved_item_page(driver: &WebDriver) -> Result<(), Report> {
    let summary = driver
        .execute(
            r#"
            const summary = [
                `marker=${window.__patchbayLabelAddMarker ?? '<missing>'}`,
                `sameUrl=${window.location.href === window.__patchbayLabelAddUrl}`,
                `scrollKept=${window.scrollY > 1000}`,
            ].join(';');
            document.body.style.minHeight = '';
            return summary;
            "#,
            Vec::new(),
        )
        .await
        .context("failed to inspect item label add background save")?
        .convert::<String>()
        .context("failed to read item label add background save summary")?;
    assert_that!(summary).is_equal_to("marker=alive;sameUrl=true;scrollKept=true".to_owned());
    Ok(())
}

async fn assert_state_label_dropdown_and_move(driver: &WebDriver) -> Result<(), Report> {
    let summary = driver
        .execute(
            r#"
            const form = document.querySelector('.label-row form.state-label-form');
            if (!form) {
                throw new Error('missing state label form');
            }
            const key = form.querySelector('input[name="key"]');
            const valueSelect = form.querySelector('select[name="value"]');
            const valueInput = form.querySelector('input[name="value"]');
            if (!valueSelect) {
                throw new Error('missing state label select');
            }
            return [
                `key=${key?.value ?? '<missing>'}`,
                `value=${valueSelect.value}`,
                `hasValueInput=${Boolean(valueInput)}`,
                `options=${Array.from(valueSelect.options)
                    .map(option => `${option.value}:${(option.textContent ?? '').trim()}`)
                    .join('|')}`,
            ].join(';');
            "#,
            Vec::new(),
        )
        .await
        .context("failed to inspect state label select")?
        .convert::<String>()
        .context("failed to read state label select summary")?;
    assert_that!(summary).is_equal_to(
        "key=state;value=in_progress;hasValueInput=false;options=idea:Idea|open:Open|in_progress:In progress|done:Done"
            .to_owned(),
    );

    driver
        .execute(
            r#"
            const form = document.querySelector('.label-row form.state-label-form');
            const valueSelect = form?.querySelector('select[name="value"]');
            if (!form || !valueSelect) {
                throw new Error('missing state label form');
            }
            window.__patchbayLabelSaveMarker = 'alive';
            window.__patchbayLabelSaveUrl = window.location.href;
            document.body.style.minHeight = '5000px';
            window.scrollTo(0, 1600);
            valueSelect.value = 'done';
            form.requestSubmit();
            "#,
            Vec::new(),
        )
        .await
        .context("failed to submit state label move")?;
    find(driver, By::XPath("//*[contains(text(), 'state=done')]")).await?;
    let save_summary = driver
        .execute(
            r#"
            const summary = [
                `marker=${window.__patchbayLabelSaveMarker ?? '<missing>'}`,
                `sameUrl=${window.location.href === window.__patchbayLabelSaveUrl}`,
                `scrollKept=${window.scrollY > 1000}`,
            ].join(';');
            document.body.style.minHeight = '';
            return summary;
            "#,
            Vec::new(),
        )
        .await
        .context("failed to inspect item label background save")?
        .convert::<String>()
        .context("failed to read item label background save summary")?;
    assert_that!(save_summary).is_equal_to("marker=alive;sameUrl=true;scrollKept=true".to_owned());
    Ok(())
}

async fn set_input_value(driver: &WebDriver, selector: &str, value: &str) -> Result<(), Report> {
    let script = format!(
        r#"
        const done = arguments[0];
        const selector = {selector:?};
        const value = {value:?};
        const deadline = Date.now() + 5000;
        const editable = (field) => {{
            if (!field) {{
                return null;
            }}
            if (field.matches('input, textarea, select')) {{
                return field;
            }}
            return field.querySelector('input, textarea, select');
        }};
        const findInput = () => {{
            const controls = Array.from(document.querySelectorAll(selector))
                .map(editable)
                .filter(Boolean);
            return controls.find((control) =>
                !control.disabled &&
                !control.readOnly &&
                control.type !== 'hidden'
            ) ?? controls[0] ?? null;
        }};
        const setValue = () => {{
            const input = findInput();
            if (!input) {{
                if (Date.now() > deadline) {{
                    done('missing input ' + selector);
                    return;
                }}
                setTimeout(setValue, 100);
                return;
            }}
            input.value = value;
            input.setAttribute('value', value);
            input.dispatchEvent(new Event('input', {{ bubbles: true }}));
            input.dispatchEvent(new Event('change', {{ bubbles: true }}));
            done('ok');
        }};
        setValue();
        "#
    );
    let result = driver
        .execute_async(script, Vec::new())
        .await
        .context("failed to set browser-test input value")?
        .convert::<String>()
        .context("failed to read browser-test input set result")?;
    if result != "ok" {
        bail!("failed to set browser-test input value: {result}");
    }
    Ok(())
}

async fn send_keys(driver: &WebDriver, by: By, value: &str) -> Result<(), Report> {
    find(driver, by)
        .await?
        .send_keys(value)
        .await
        .context("failed to type into browser-test element")?;
    Ok(())
}
