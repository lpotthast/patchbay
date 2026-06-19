use super::*;

#[component]
pub(crate) fn PersonalitiesPanel(
    api_base_url: String,
    project: String,
    project_id: i64,
) -> impl IntoView + 'static {
    view! {
        <section id="personalities" class="personalities-admin panel">
            <div class="panel-heading">
                <h2>"Personalities"</h2>
            </div>
            <div class="crudkit-personalities" data-crudkit-leptos="personalities">
                {personalities_crudkit_instance(api_base_url, project, project_id)}
            </div>
        </section>
    }
}

fn personalities_crudkit_instance(
    api_base_url: String,
    project: String,
    project_id: i64,
) -> impl IntoView + 'static {
    let (context, set_context) = signal(None::<CrudInstanceContext>);
    reload_crudkit_on_live_event(context, move |event| {
        event_scopes_named_project(event, Some(project.as_str()))
            && matches!(event, UiEvent::AutomationChanged { .. })
    });

    view! {
        <CrudInstance
            name="personalities"
            config=personalities_crudkit_config(api_base_url, project_id)
            on_context_created=Callback::new(move |context| set_context.set(Some(context)))
        />
    }
}

fn personalities_crudkit_config(api_base_url: String, project_id: i64) -> CrudInstanceConfig {
    CrudInstanceConfig {
        api_base_url,
        view: SerializableCrudView::List,
        list_columns: vec![
            Header::showing(
                ReadPersonalityField::Name,
                HeaderOptions {
                    display_name: "Name".into(),
                    ..Default::default()
                },
            ),
            Header::showing(
                ReadPersonalityField::UpdatedAt,
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
                    CreatePersonalityField::Name,
                    FieldOptions {
                        label: Some(Label::new("Name")),
                        ..Default::default()
                    },
                ),
                Elem::create_field(
                    CreatePersonalityField::PersonalityDescription,
                    FieldOptions {
                        label: Some(Label::new("Personality description")),
                        ..Default::default()
                    },
                ),
            ],
        }))]),
        elements: vec![Elem::Enclosing(Enclosing::None(Group {
            layout: Layout::default(),
            children: vec![
                Elem::field(
                    Personality::Id,
                    FieldOptions {
                        disabled: true,
                        label: Some(Label::new("ID")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    PersonalityField::Name,
                    FieldOptions {
                        label: Some(Label::new("Name")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    PersonalityField::PersonalityDescription,
                    FieldOptions {
                        label: Some(Label::new("Personality description")),
                        ..Default::default()
                    },
                ),
            ],
        }))],
        order_by: indexmap! {
            ReadPersonality::Name.into() => Order::Asc,
        },
        items_per_page: ItemsPerPage::default(),
        page_nr: PageNr::first(),
        base_condition: Some(project_id_condition(project_id)),
        resource_name: CrudPersonalityResource::resource_name().to_owned(),
        reqwest_executor: Arc::new(NewClientPerRequestExecutor),
        model_handler: personality_model_handler(project_id),
        actions: vec![],
        entity_actions: vec![],
        navigation: CrudNavigationConfig::default(),
        read_field_renderer: FieldRendererRegistry::builder().build(),
        create_field_renderer: FieldRendererRegistry::builder()
            .register(
                CreatePersonalityField::PersonalityDescription,
                multiline_text_field_renderer::<DynCreateField>("Personality description"),
            )
            .build(),
        update_field_renderer: FieldRendererRegistry::builder()
            .register(
                PersonalityField::PersonalityDescription,
                multiline_text_field_renderer::<DynUpdateField>("Personality description"),
            )
            .build(),
    }
}

fn personality_model_handler(project_id: i64) -> ModelHandler {
    let mut handler = ModelHandler::new::<CreatePersonality, ReadPersonality, Personality>();
    handler.get_default_create_model = Callback::new(move |()| {
        DynCreateModel::from(CreatePersonality {
            project_id,
            ..Default::default()
        })
    });
    handler
}
