use super::*;

#[component]
pub(crate) fn SwimLanesPanel(
    api_base_url: String,
    project: String,
    project_id: i64,
    edit_lane_id: Option<i64>,
) -> impl IntoView + 'static {
    view! {
        <section id="swim-lanes" class="swim-lanes-admin panel">
            <div class="panel-heading">
                <h2>"Swim-lanes"</h2>
            </div>
            <div class="crudkit-swim-lanes" data-crudkit-leptos="swim-lanes">
                {swim_lanes_crudkit_instance(api_base_url, project, project_id, edit_lane_id)}
            </div>
        </section>
    }
}

fn swim_lanes_crudkit_instance(
    api_base_url: String,
    project: String,
    project_id: i64,
    edit_lane_id: Option<i64>,
) -> impl IntoView + 'static {
    let (context, set_context) = signal(None::<CrudInstanceContext>);
    reload_crudkit_on_live_event(context, move |event| {
        event_scopes_named_project(event, Some(project.as_str()))
            && matches!(event, UiEvent::SwimLaneChanged { .. })
    });

    view! {
        <CrudInstance
            name="swim-lanes"
            config=swim_lanes_crudkit_config(api_base_url, project_id, edit_lane_id)
            on_context_created=Callback::new(move |context| set_context.set(Some(context)))
        />
    }
}

fn swim_lanes_crudkit_config(
    api_base_url: String,
    project_id: i64,
    edit_lane_id: Option<i64>,
) -> CrudInstanceConfig {
    let view = edit_lane_id
        .map(crudkit_i64_id)
        .map(SerializableCrudView::Edit)
        .unwrap_or(SerializableCrudView::List);
    CrudInstanceConfig {
        api_base_url,
        view,
        list_columns: vec![
            Header::showing(
                ReadSwimLaneField::Identifier,
                HeaderOptions {
                    display_name: "Identifier".into(),
                    ..Default::default()
                },
            ),
            Header::showing(
                ReadSwimLaneField::Name,
                HeaderOptions {
                    display_name: "Name".into(),
                    ..Default::default()
                },
            ),
            Header::showing(
                ReadSwimLaneField::Position,
                HeaderOptions {
                    display_name: "Position".into(),
                    min_width: true,
                    ..Default::default()
                },
            ),
            Header::showing(
                ReadSwimLaneField::ItemOrder,
                HeaderOptions {
                    display_name: "Order".into(),
                    min_width: true,
                    ..Default::default()
                },
            ),
            Header::showing(
                ReadSwimLaneField::CanCreateItems,
                HeaderOptions {
                    display_name: "Can create items".into(),
                    min_width: true,
                    ..Default::default()
                },
            ),
        ],
        create_elements: CreateElements::Custom(vec![Elem::Enclosing(Enclosing::None(Group {
            layout: Layout::default(),
            children: vec![
                Elem::create_field(
                    CreateSwimLaneField::Identifier,
                    FieldOptions {
                        label: Some(Label::new("Identifier")),
                        ..Default::default()
                    },
                ),
                Elem::create_field(
                    CreateSwimLaneField::Name,
                    FieldOptions {
                        label: Some(Label::new("Name")),
                        ..Default::default()
                    },
                ),
                Elem::create_field(
                    CreateSwimLaneField::Position,
                    FieldOptions {
                        label: Some(Label::new("Position")),
                        ..Default::default()
                    },
                ),
                Elem::create_field(
                    CreateSwimLaneField::Filter,
                    FieldOptions {
                        label: Some(Label::new("Filter")),
                        ..Default::default()
                    },
                ),
                Elem::create_field(
                    CreateSwimLaneField::ItemOrder,
                    FieldOptions {
                        label: Some(Label::new("Order")),
                        ..Default::default()
                    },
                ),
                Elem::create_field(
                    CreateSwimLaneField::CanCreateItems,
                    FieldOptions {
                        label: Some(Label::new("Can create items")),
                        ..Default::default()
                    },
                ),
            ],
        }))]),
        elements: vec![Elem::Enclosing(Enclosing::None(Group {
            layout: Layout::default(),
            children: vec![
                Elem::field(
                    SwimLane::Id,
                    FieldOptions {
                        disabled: true,
                        label: Some(Label::new("ID")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    SwimLaneField::Identifier,
                    FieldOptions {
                        label: Some(Label::new("Identifier")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    SwimLaneField::Name,
                    FieldOptions {
                        label: Some(Label::new("Name")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    SwimLaneField::Position,
                    FieldOptions {
                        label: Some(Label::new("Position")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    SwimLaneField::Filter,
                    FieldOptions {
                        label: Some(Label::new("Filter")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    SwimLaneField::ItemOrder,
                    FieldOptions {
                        label: Some(Label::new("Order")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    SwimLaneField::CanCreateItems,
                    FieldOptions {
                        label: Some(Label::new("Can create items")),
                        ..Default::default()
                    },
                ),
            ],
        }))],
        order_by: indexmap! {
            ReadSwimLane::Position.into() => Order::Asc,
            ReadSwimLane::Id.into() => Order::Asc,
        },
        items_per_page: ItemsPerPage::default(),
        page_nr: PageNr::first(),
        base_condition: Some(project_id_condition(project_id)),
        resource_name: CrudSwimLaneResource::resource_name().to_owned(),
        reqwest_executor: Arc::new(NewClientPerRequestExecutor),
        model_handler: swim_lane_model_handler(project_id),
        actions: vec![],
        entity_actions: vec![],
        navigation: CrudNavigationConfig::default(),
        read_field_renderer: FieldRendererRegistry::builder().build(),
        create_field_renderer: FieldRendererRegistry::builder()
            .register(
                CreateSwimLaneField::Filter,
                multiline_text_field_renderer::<DynCreateField>(
                    "{\"All\":[{\"column_name\":\"state\",\"operator\":\"=\",\"value\":{\"String\":\"open\"}}]}",
                ),
            )
            .register(
                CreateSwimLaneField::ItemOrder,
                swim_lane_order_field_renderer::<DynCreateField>(),
            )
            .build(),
        update_field_renderer: FieldRendererRegistry::builder()
            .register(
                SwimLaneField::Filter,
                multiline_text_field_renderer::<DynUpdateField>(
                    "{\"All\":[{\"column_name\":\"state\",\"operator\":\"=\",\"value\":{\"String\":\"open\"}}]}",
                ),
            )
            .register(
                SwimLaneField::ItemOrder,
                swim_lane_order_field_renderer::<DynUpdateField>(),
            )
            .build(),
    }
}

fn swim_lane_model_handler(project_id: i64) -> ModelHandler {
    let mut handler = ModelHandler::new::<CreateSwimLane, ReadSwimLane, SwimLane>();
    handler.get_default_create_model = Callback::new(move |()| {
        DynCreateModel::from(CreateSwimLane {
            project_id,
            position: 50,
            filter: "{\"All\":[]}".to_owned(),
            item_order: "updated_desc".to_owned(),
            ..Default::default()
        })
    });
    handler
}
