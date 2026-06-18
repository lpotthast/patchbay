use super::*;

pub(crate) fn projects_panel(api_base_url: String) -> impl IntoView + 'static {
    view! {
        <section class="project-management panel">
            <div class="panel-heading">
                <h2>"Projects"</h2>
            </div>
            <div class="crudkit-projects" data-crudkit-leptos="projects">
                {projects_crudkit_instance(api_base_url)}
            </div>
        </section>
    }
}

fn projects_crudkit_instance(api_base_url: String) -> impl IntoView + 'static {
    let (context, set_context) = signal(None::<CrudInstanceContext>);
    reload_crudkit_on_live_event(context, |event| {
        matches!(
            event,
            UiEvent::ProjectListChanged { .. } | UiEvent::ProjectChanged { .. }
        )
    });

    view! {
        <CrudInstance
            name="projects"
            config=projects_crudkit_config(api_base_url)
            on_context_created=Callback::new(move |context| set_context.set(Some(context)))
        />
    }
}

fn projects_crudkit_config(api_base_url: String) -> CrudInstanceConfig {
    CrudInstanceConfig {
        api_base_url,
        view: SerializableCrudView::List,
        list_columns: vec![
            Header::showing(
                ReadProjectField::Id,
                HeaderOptions {
                    display_name: "#".into(),
                    min_width: true,
                    ..Default::default()
                },
            ),
            Header::showing(
                ReadProjectField::Name,
                HeaderOptions {
                    display_name: "Project key".into(),
                    ..Default::default()
                },
            ),
            Header::showing(
                ReadProjectField::DisplayName,
                HeaderOptions {
                    display_name: "Display name".into(),
                    ..Default::default()
                },
            ),
            Header::showing(
                ReadProjectField::Path,
                HeaderOptions {
                    display_name: "Path".into(),
                    ..Default::default()
                },
            ),
            Header::showing(
                ReadProjectField::PathExists,
                HeaderOptions {
                    display_name: "Path status".into(),
                    min_width: true,
                    ..Default::default()
                },
            ),
            Header::showing(
                ReadProjectField::WorkspaceMode,
                HeaderOptions {
                    display_name: "Workspace".into(),
                    ..Default::default()
                },
            ),
            Header::showing(
                ReadProjectField::DefaultAgentModel,
                HeaderOptions {
                    display_name: "Model".into(),
                    ..Default::default()
                },
            ),
            Header::showing(
                ReadProjectField::DefaultAgentReasoningEffort,
                HeaderOptions {
                    display_name: "Reasoning".into(),
                    ..Default::default()
                },
            ),
            Header::showing(
                ReadProjectField::UpdatedAt,
                HeaderOptions {
                    display_name: "Updated".into(),
                    ..Default::default()
                },
            ),
        ],
        create_elements: CreateElements::Custom(vec![Elem::Enclosing(Enclosing::None(Group {
            layout: Layout::default(),
            children: vec![
                Elem::create_field(
                    CreateProjectField::Name,
                    FieldOptions {
                        label: Some(Label::new("Project key")),
                        ..Default::default()
                    },
                ),
                Elem::create_field(
                    CreateProjectField::DisplayName,
                    FieldOptions {
                        label: Some(Label::new("Display name")),
                        ..Default::default()
                    },
                ),
                Elem::create_field(
                    CreateProjectField::Path,
                    FieldOptions {
                        label: Some(Label::new("Path")),
                        ..Default::default()
                    },
                ),
                Elem::create_field(
                    CreateProjectField::DefaultAgentModel,
                    FieldOptions {
                        label: Some(Label::new("Default model")),
                        ..Default::default()
                    },
                ),
                Elem::create_field(
                    CreateProjectField::DefaultAgentReasoningEffort,
                    FieldOptions {
                        label: Some(Label::new("Default reasoning")),
                        ..Default::default()
                    },
                ),
                Elem::create_field(
                    CreateProjectField::Memory,
                    FieldOptions {
                        label: Some(Label::new("Memory")),
                        ..Default::default()
                    },
                ),
            ],
        }))]),
        elements: vec![Elem::Enclosing(Enclosing::None(Group {
            layout: Layout::default(),
            children: vec![
                Elem::field(
                    CrudProject::Id,
                    FieldOptions {
                        disabled: true,
                        label: Some(Label::new("ID")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    ProjectField::DisplayName,
                    FieldOptions {
                        label: Some(Label::new("Display name")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    ProjectField::Path,
                    FieldOptions {
                        label: Some(Label::new("Path")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    ProjectField::Memory,
                    FieldOptions {
                        label: Some(Label::new("Memory")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    ProjectField::WorkspaceMode,
                    FieldOptions {
                        label: Some(Label::new("Workspace")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    ProjectField::MaxCodeEditAgents,
                    FieldOptions {
                        label: Some(Label::new("Max agents")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    ProjectField::MaxReadOnlyAgents,
                    FieldOptions {
                        label: Some(Label::new("Read-only agents")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    ProjectField::CreatePr,
                    FieldOptions {
                        label: Some(Label::new("Create PR")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    ProjectField::AutoCommit,
                    FieldOptions {
                        label: Some(Label::new("Auto-Commit")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    ProjectField::CommitStandard,
                    FieldOptions {
                        label: Some(Label::new("Commit standard")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    ProjectField::RevertStrategy,
                    FieldOptions {
                        label: Some(Label::new("Failure revert")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    ProjectField::StaleClaimMinutes,
                    FieldOptions {
                        label: Some(Label::new("Stale claim minutes")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    ProjectField::WorktreeCleanupPolicy,
                    FieldOptions {
                        label: Some(Label::new("Worktree cleanup")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    ProjectField::DefaultAgentTool,
                    FieldOptions {
                        label: Some(Label::new("Default tool")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    ProjectField::DefaultAgentModel,
                    FieldOptions {
                        label: Some(Label::new("Default model")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    ProjectField::DefaultAgentReasoningEffort,
                    FieldOptions {
                        label: Some(Label::new("Default reasoning")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    ProjectField::AgentSandboxMode,
                    FieldOptions {
                        label: Some(Label::new("Sandbox mode")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    ProjectField::AgentExtraWritableRoots,
                    FieldOptions {
                        label: Some(Label::new("Extra writable roots")),
                        ..Default::default()
                    },
                ),
            ],
        }))],
        order_by: indexmap! {
            ReadProject::Name.into() => Order::Asc,
        },
        items_per_page: ItemsPerPage::default(),
        page_nr: PageNr::first(),
        base_condition: None,
        resource_name: CrudProjectResource::resource_name().to_owned(),
        reqwest_executor: Arc::new(NewClientPerRequestExecutor),
        model_handler: ModelHandler::new::<CreateProject, ReadProject, CrudProject>(),
        actions: vec![],
        entity_actions: vec![],
        navigation: CrudNavigationConfig::default(),
        read_field_renderer: FieldRendererRegistry::builder()
            .register(
                ReadProjectField::PathExists,
                project_path_status_renderer::<DynReadField>(),
            )
            .register(
                ReadProjectField::DefaultAgentModel,
                agent_model_field_renderer::<DynReadField>(Some("Codex default")),
            )
            .build(),
        create_field_renderer: FieldRendererRegistry::builder()
            .register(
                CreateProjectField::Path,
                project_path_field_renderer::<DynCreateField>(),
            )
            .register(
                CreateProjectField::DefaultAgentModel,
                agent_model_field_renderer::<DynCreateField>(None),
            )
            .register(
                CreateProjectField::DefaultAgentReasoningEffort,
                agent_reasoning_field_renderer::<DynCreateField>(None),
            )
            .build(),
        update_field_renderer: FieldRendererRegistry::builder()
            .register(
                ProjectField::Path,
                project_path_field_renderer::<DynUpdateField>(),
            )
            .register(
                ProjectField::WorkspaceMode,
                select_field_renderer::<DynUpdateField>(
                    &[
                        ("current_branch", "current_branch"),
                        ("git_worktree", "git_worktree"),
                        ("git_branch", "git_branch"),
                    ],
                    false,
                ),
            )
            .register(
                ProjectField::WorktreeCleanupPolicy,
                select_field_renderer::<DynUpdateField>(
                    &[("manual", "manual"), ("after_success", "after_success")],
                    false,
                ),
            )
            .register(
                ProjectField::RevertStrategy,
                select_field_renderer::<DynUpdateField>(
                    &[("manual", "manual"), ("git_reset", "git_reset")],
                    false,
                ),
            )
            .register(
                ProjectField::DefaultAgentTool,
                select_field_renderer::<DynUpdateField>(&[("codex", "codex")], false),
            )
            .register(
                ProjectField::DefaultAgentModel,
                agent_model_field_renderer::<DynUpdateField>(Some("Codex default")),
            )
            .register(
                ProjectField::DefaultAgentReasoningEffort,
                agent_reasoning_field_renderer::<DynUpdateField>(Some("Codex default")),
            )
            .register(
                ProjectField::AgentSandboxMode,
                select_field_renderer::<DynUpdateField>(
                    &[
                        ("workspace_write", "workspace_write"),
                        ("danger_full_access", "danger_full_access"),
                    ],
                    false,
                ),
            )
            .register(
                ProjectField::AgentExtraWritableRoots,
                multiline_text_field_renderer::<DynUpdateField>(
                    "One absolute path per line; ~ is expanded on save.",
                ),
            )
            .build(),
    }
}
