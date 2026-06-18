use super::*;

#[component]
pub(crate) fn WorkItemStatesPanel(
    api_base_url: String,
    project: String,
    project_id: i64,
) -> impl IntoView + 'static {
    view! {
        <section id="work-item-states" class="work-item-states-admin panel">
            <div class="panel-heading">
                <h2>"Work item states"</h2>
            </div>
            <div class="crudkit-work-item-states" data-crudkit-leptos="work-item-states">
                {work_item_states_crudkit_instance(api_base_url, project, project_id)}
            </div>
        </section>
    }
}

fn work_item_states_crudkit_instance(
    api_base_url: String,
    project: String,
    project_id: i64,
) -> impl IntoView + 'static {
    let (context, set_context) = signal(None::<CrudInstanceContext>);
    reload_crudkit_on_live_event(context, move |event| {
        event_scopes_named_project(event, Some(project.as_str()))
            && matches!(event, UiEvent::WorkItemStateChanged { .. })
    });

    view! {
        <CrudInstance
            name="work-item-states"
            config=work_item_states_crudkit_config(api_base_url, project_id)
            on_context_created=Callback::new(move |context| set_context.set(Some(context)))
        />
    }
}

fn work_item_states_crudkit_config(api_base_url: String, project_id: i64) -> CrudInstanceConfig {
    CrudInstanceConfig {
        api_base_url,
        view: SerializableCrudView::List,
        list_columns: vec![
            Header::showing(
                ReadWorkItemStateField::Identifier,
                HeaderOptions {
                    display_name: "Identifier".into(),
                    ..Default::default()
                },
            ),
            Header::showing(
                ReadWorkItemStateField::Name,
                HeaderOptions {
                    display_name: "Name".into(),
                    ..Default::default()
                },
            ),
            Header::showing(
                ReadWorkItemStateField::Position,
                HeaderOptions {
                    display_name: "Position".into(),
                    min_width: true,
                    ..Default::default()
                },
            ),
        ],
        create_elements: CreateElements::Custom(vec![Elem::Enclosing(Enclosing::None(Group {
            layout: Layout::default(),
            children: vec![
                Elem::create_field(
                    CreateWorkItemStateField::Identifier,
                    FieldOptions {
                        label: Some(Label::new("Identifier")),
                        ..Default::default()
                    },
                ),
                Elem::create_field(
                    CreateWorkItemStateField::Name,
                    FieldOptions {
                        label: Some(Label::new("Name")),
                        ..Default::default()
                    },
                ),
                Elem::create_field(
                    CreateWorkItemStateField::Position,
                    FieldOptions {
                        label: Some(Label::new("Position")),
                        ..Default::default()
                    },
                ),
            ],
        }))]),
        elements: vec![Elem::Enclosing(Enclosing::None(Group {
            layout: Layout::default(),
            children: vec![
                Elem::field(
                    WorkItemState::Id,
                    FieldOptions {
                        disabled: true,
                        label: Some(Label::new("ID")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    WorkItemStateField::Identifier,
                    FieldOptions {
                        label: Some(Label::new("Identifier")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    WorkItemStateField::Name,
                    FieldOptions {
                        label: Some(Label::new("Name")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    WorkItemStateField::Position,
                    FieldOptions {
                        label: Some(Label::new("Position")),
                        ..Default::default()
                    },
                ),
            ],
        }))],
        order_by: indexmap! {
            ReadWorkItemState::Position.into() => Order::Asc,
            ReadWorkItemState::Id.into() => Order::Asc,
        },
        items_per_page: ItemsPerPage::default(),
        page_nr: PageNr::first(),
        base_condition: Some(project_id_condition(project_id)),
        resource_name: CrudWorkItemStateResource::resource_name().to_owned(),
        reqwest_executor: Arc::new(NewClientPerRequestExecutor),
        model_handler: work_item_state_model_handler(project_id),
        actions: vec![],
        entity_actions: vec![],
        navigation: CrudNavigationConfig::default(),
        read_field_renderer: FieldRendererRegistry::builder().build(),
        create_field_renderer: FieldRendererRegistry::builder().build(),
        update_field_renderer: FieldRendererRegistry::builder().build(),
    }
}

fn work_item_state_model_handler(project_id: i64) -> ModelHandler {
    let mut handler = ModelHandler::new::<CreateWorkItemState, ReadWorkItemState, WorkItemState>();
    handler.get_default_create_model = Callback::new(move |()| {
        DynCreateModel::from(CreateWorkItemState {
            project_id,
            position: 50,
            ..Default::default()
        })
    });
    handler
}
