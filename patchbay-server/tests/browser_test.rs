#![cfg(not(target_arch = "wasm32"))]

use std::{borrow::Cow, env, time::Duration};

use assertr::prelude::*;
use browser_test::thirtyfour::{By, ChromiumLikeCapabilities, WebDriver};
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

        let app = LeptosTestAppConfig::new(env!("CARGO_MANIFEST_DIR"))
            .with_app_name("patchbay browser test")
            .with_forward_logs(true)
            .with_startup_line("Serving Patchbay")
            .with_env("PATCHBAY_DATABASE", database.as_os_str())
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
        seed_memory_history(driver).await?;
        driver
            .goto(app.url("/?project=demo"))
            .await
            .context("failed to open Patchbay board page")?;

        find(driver, By::Css("section.project-settings")).await?;
        find(driver, By::Css("section.board")).await?;
        find(driver, By::Css(".workspace-panel .workspace-actions")).await?;
        assert_that!(driver.title().await.context("failed to read page title")?)
            .is_equal_to("Patchbay");
        assert_source_contains(driver, "Copy path").await?;
        assert_source_contains(driver, "Open folder").await?;
        assert_source_contains(driver, "Open IDE").await?;
        assert_source_contains(driver, "System prompt").await?;
        assert_source_does_not_contain(driver, "project-option-key").await?;
        assert_source_contains(driver, "Memory").await?;
        assert_source_contains(driver, "Automation policy").await?;
        assert_source_contains(driver, "Auto-Commit").await?;
        find(driver, By::Css("#project-auto-commit")).await?;
        find(driver, By::Css("#project-commit-standard")).await?;
        find(
            driver,
            By::Css("#project-revert-strategy option[value='git_reset']"),
        )
        .await?;
        assert_source_contains(driver, "memory history").await?;
        assert_source_does_not_contain(driver, "Compact history").await?;
        assert_source_does_not_contain(driver, "Append memory").await?;
        assert_source_does_not_contain(driver, "append-memory").await?;
        assert_source_does_not_contain(driver, "/memory/append").await?;
        assert_source_does_not_contain(driver, "memory-history-entry").await?;
        assert_source_does_not_contain(driver, "memory-snapshot").await?;
        find(driver, By::Css("#project-memory-version")).await?;
        find(driver, By::Css("textarea.project-memory-text")).await?;
        assert_memory_history_selector_behaviour(driver).await?;
        assert_source_does_not_contain(driver, "Run settings").await?;
        assert_source_contains(driver, "Runs").await?;
        assert_source_contains(driver, "No runs yet").await?;
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
        assert_crudkit_create_form_survives_live_event(driver).await?;

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
        driver
            .goto(app.url("/?project=demo"))
            .await
            .context("failed to reopen Patchbay board page after API check")?;
        open_new_item_modal(driver).await?;
        find(driver, By::Css("#new-item-modal select[name='state']")).await?;
        assert_new_item_lane_options(driver).await?;
        find(
            driver,
            By::Css("#new-item-modal select[name='agent_model_override']"),
        )
        .await?;
        assert_source_contains(driver, "Project default").await?;
        send_keys(
            driver,
            By::Css("#new-item-modal input[name='title']"),
            "Browser item",
        )
        .await?;
        send_keys(
            driver,
            By::Css("#new-item-modal textarea[name='description']"),
            "Created through browser-test",
        )
        .await?;
        click(driver, By::Css("#new-item-modal button[type='submit']")).await?;

        find(driver, By::LinkText("Browser item")).await?;
        assert_source_contains(driver, "Created through browser-test").await?;
        assert_source_contains(driver, "state=idea").await?;

        click(driver, By::LinkText("Browser item")).await?;
        find(driver, By::Css("section.item-settings")).await?;
        find(driver, By::Css("section.comments")).await?;
        assert_source_contains(driver, "Item details").await?;
        assert_source_does_not_contain(driver, "automation can claim this item").await?;
        assert_source_contains(driver, "Start agent").await?;
        assert_source_contains(driver, "Comments").await?;
        add_agent_comment(driver).await?;
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
        assert_source_contains(driver, "patchbay-run-60").await?;
        find(driver, By::Css("section.item-labels")).await?;
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
                        mode: 'refine',
                        tool_name: 'codex',
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
    for _ in 0..20 {
        click(driver, By::Css("section.board-toolbar > button")).await?;
        if driver
            .find(By::Css("leptonic-modal#new-item-modal"))
            .await
            .is_ok()
        {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    find(driver, By::Css("leptonic-modal#new-item-modal"))
        .await
        .map(|_| ())
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

async fn assert_source_contains(driver: &WebDriver, expected: &str) -> Result<(), Report> {
    let source = driver
        .source()
        .await
        .context("failed to read page source")?;
    assert_that!(source).contains(expected);
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
            form.submit();
            "#,
            Vec::new(),
        )
        .await
        .context("failed to submit label add form")?;
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
