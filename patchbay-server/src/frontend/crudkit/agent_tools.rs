use super::*;

pub(crate) fn agent_tools_panel(api_base_url: String) -> impl IntoView + 'static {
    view! {
        <section class="app-tools panel">
            <div class="panel-heading">
                <h2>"Codex app-server"</h2>
                <p class="muted">"Patchbay requires Codex app-server for automation."</p>
            </div>
            <form method="post" action="/agent-tools/discover">
                <input type="hidden" name="return_to" value="/projects"/>
                <button>"Check Codex"</button>
            </form>
            <div class="crudkit-agent-tools" data-crudkit-leptos="agent-tools">
                {agent_tools_crudkit_instance(api_base_url)}
            </div>
        </section>
    }
}

fn agent_tools_crudkit_instance(api_base_url: String) -> impl IntoView + 'static {
    let (context, set_context) = signal(None::<CrudInstanceContext>);
    reload_crudkit_on_live_event(context, |event| {
        matches!(
            event,
            UiEvent::AgentToolChanged { .. } | UiEvent::CodexStatusChanged { .. }
        )
    });

    view! {
        <CrudInstance
            name="agent-tools"
            config=agent_tools_crudkit_config(api_base_url)
            on_context_created=Callback::new(move |context| set_context.set(Some(context)))
        />
    }
}

fn agent_tools_crudkit_config(api_base_url: String) -> CrudInstanceConfig {
    CrudInstanceConfig {
        api_base_url,
        view: SerializableCrudView::List,
        list_columns: vec![
            Header::showing(
                ReadAgentToolField::Id,
                HeaderOptions {
                    display_name: "#".into(),
                    min_width: true,
                    ..Default::default()
                },
            ),
            Header::showing(
                ReadAgentToolField::ToolName,
                HeaderOptions {
                    display_name: "Tool".into(),
                    ..Default::default()
                },
            ),
            Header::showing(
                ReadAgentToolField::ExecutablePath,
                HeaderOptions {
                    display_name: "Configured binary".into(),
                    ..Default::default()
                },
            ),
            Header::showing(
                ReadAgentToolField::DiscoveredPath,
                HeaderOptions {
                    display_name: "Discovered binary".into(),
                    ..Default::default()
                },
            ),
        ],
        create_elements: CreateElements::Custom(vec![Elem::Enclosing(Enclosing::Card(Group {
            layout: Layout::default(),
            children: vec![
                Elem::create_field(
                    CreateAgentToolField::ToolName,
                    FieldOptions {
                        label: Some(Label::new("Tool")),
                        ..Default::default()
                    },
                ),
                Elem::create_field(
                    CreateAgentToolField::ExecutablePath,
                    FieldOptions {
                        label: Some(Label::new("Codex binary path")),
                        ..Default::default()
                    },
                ),
            ],
        }))]),
        elements: vec![Elem::Enclosing(Enclosing::Card(Group {
            layout: Layout::default(),
            children: vec![
                Elem::field(
                    AgentTool::Id,
                    FieldOptions {
                        disabled: true,
                        label: Some(Label::new("ID")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    AgentToolField::ExecutablePath,
                    FieldOptions {
                        label: Some(Label::new("Executable path")),
                        ..Default::default()
                    },
                ),
            ],
        }))],
        order_by: indexmap! {
            ReadAgentTool::Id.into() => Order::Asc,
        },
        items_per_page: ItemsPerPage::default(),
        page_nr: PageNr::first(),
        base_condition: None,
        resource_name: CrudAgentToolResource::resource_name().to_owned(),
        reqwest_executor: Arc::new(NewClientPerRequestExecutor),
        model_handler: ModelHandler::new::<CreateAgentTool, ReadAgentTool, AgentTool>(),
        actions: vec![],
        entity_actions: vec![],
        navigation: CrudNavigationConfig::default(),
        read_field_renderer: FieldRendererRegistry::builder().build(),
        create_field_renderer: FieldRendererRegistry::builder().build(),
        update_field_renderer: FieldRendererRegistry::builder().build(),
    }
}
