use super::*;

#[derive(Clone, Copy)]
pub(crate) enum AutomationTableKind {
    Consuming,
    Producing,
}

impl AutomationTableKind {
    fn instance_name(self) -> &'static str {
        match self {
            Self::Consuming => "work-consuming-automations",
            Self::Producing => "work-producing-automations",
        }
    }

    fn effect(self) -> &'static str {
        match self {
            Self::Consuming => "consume_work",
            Self::Producing => "produce_work",
        }
    }

    fn default_activation(self) -> &'static str {
        match self {
            Self::Consuming => "work_item",
            Self::Producing => "manual",
        }
    }

    fn default_selector(self) -> Option<String> {
        match self {
            Self::Consuming => CreateAutomationTrigger::default().work_item_selector,
            Self::Producing => None,
        }
    }

    fn activation_choices(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Consuming => &[
                ("manual", "manual"),
                ("work_item", "work_item"),
                ("work_item_created", "work_item_created"),
                ("cron", "cron"),
            ],
            Self::Producing => &[("manual", "manual"), ("cron", "cron")],
        }
    }
}

pub(crate) fn automation_triggers_crudkit_instance(
    api_base_url: String,
    project: String,
    project_id: i64,
    kind: AutomationTableKind,
    on_context_created: Callback<CrudInstanceContext>,
) -> impl IntoView + 'static {
    let (context, set_context) = signal(None::<CrudInstanceContext>);
    reload_crudkit_on_live_event(context, move |event| {
        event_scopes_named_project(event, Some(project.as_str()))
            && matches!(event, UiEvent::AutomationChanged { .. })
    });
    let created = Callback::new(move |context| {
        set_context.set(Some(context));
        on_context_created.run(context);
    });

    view! {
        <CrudInstance
            name=kind.instance_name()
            config=automation_triggers_crudkit_config(api_base_url, project_id, kind)
            on_context_created=created
        />
    }
}

fn automation_triggers_crudkit_config(
    api_base_url: String,
    project_id: i64,
    kind: AutomationTableKind,
) -> CrudInstanceConfig {
    let mut list_columns = vec![
        Header::showing(
            ReadAutomationTriggerField::Id,
            HeaderOptions {
                display_name: "#".into(),
                min_width: true,
                ..Default::default()
            },
        ),
        Header::showing(
            ReadAutomationTriggerField::Name,
            HeaderOptions {
                display_name: "Name".into(),
                ..Default::default()
            },
        ),
        Header::showing(
            ReadAutomationTriggerField::Activation,
            HeaderOptions {
                display_name: "Activation".into(),
                ..Default::default()
            },
        ),
        Header::showing(
            ReadAutomationTriggerField::Schedule,
            HeaderOptions {
                display_name: "Schedule".into(),
                ..Default::default()
            },
        ),
        Header::showing(
            ReadAutomationTriggerField::Enabled,
            HeaderOptions {
                display_name: "Enabled".into(),
                min_width: true,
                ..Default::default()
            },
        ),
        Header::showing(
            ReadAutomationTriggerField::Priority,
            HeaderOptions {
                display_name: "Priority".into(),
                min_width: true,
                ..Default::default()
            },
        ),
        Header::showing(
            ReadAutomationTriggerField::EvaluationCount,
            HeaderOptions {
                display_name: "Evaluations".into(),
                min_width: true,
                ..Default::default()
            },
        ),
        Header::showing(
            ReadAutomationTriggerField::PendingEvaluationCount,
            HeaderOptions {
                display_name: "Queued".into(),
                min_width: true,
                ..Default::default()
            },
        ),
        Header::showing(
            ReadAutomationTriggerField::NextEvaluationAt,
            HeaderOptions {
                display_name: "Next evaluation".into(),
                ..Default::default()
            },
        ),
    ];
    if matches!(kind, AutomationTableKind::Consuming) {
        list_columns.insert(
            4,
            Header::showing(
                ReadAutomationTriggerField::Mutability,
                HeaderOptions {
                    display_name: "Mutability".into(),
                    min_width: true,
                    ..Default::default()
                },
            ),
        );
    }
    let mut create_children = vec![
        Elem::create_field(
            CreateAutomationTriggerField::Name,
            FieldOptions {
                label: Some(Label::new("Name")),
                ..Default::default()
            },
        ),
        Elem::create_field(
            CreateAutomationTriggerField::Activation,
            FieldOptions {
                label: Some(Label::new("Activation")),
                ..Default::default()
            },
        ),
        Elem::create_field(
            CreateAutomationTriggerField::Schedule,
            FieldOptions {
                label: Some(Label::new("Schedule")),
                ..Default::default()
            },
        ),
        Elem::create_field(
            CreateAutomationTriggerField::Enabled,
            FieldOptions {
                label: Some(Label::new("Enabled")),
                ..Default::default()
            },
        ),
        Elem::create_field(
            CreateAutomationTriggerField::Priority,
            FieldOptions {
                label: Some(Label::new("Priority")),
                ..Default::default()
            },
        ),
    ];
    if matches!(kind, AutomationTableKind::Consuming) {
        create_children.push(Elem::create_field(
            CreateAutomationTriggerField::Mutability,
            FieldOptions {
                label: Some(Label::new("Mutability")),
                ..Default::default()
            },
        ));
        create_children.push(Elem::create_field(
            CreateAutomationTriggerField::WorkItemSelector,
            FieldOptions {
                label: Some(Label::new("Work item selector")),
                ..Default::default()
            },
        ));
    }
    create_children.push(Elem::create_field(
        CreateAutomationTriggerField::Prompt,
        FieldOptions {
            label: Some(Label::new("Prompt")),
            ..Default::default()
        },
    ));

    let mut update_children = vec![
        Elem::field(
            AutomationTrigger::Id,
            FieldOptions {
                disabled: true,
                label: Some(Label::new("ID")),
                ..Default::default()
            },
        ),
        Elem::field(
            AutomationTriggerField::Name,
            FieldOptions {
                label: Some(Label::new("Name")),
                ..Default::default()
            },
        ),
        Elem::field(
            AutomationTriggerField::Activation,
            FieldOptions {
                label: Some(Label::new("Activation")),
                ..Default::default()
            },
        ),
        Elem::field(
            AutomationTriggerField::Schedule,
            FieldOptions {
                label: Some(Label::new("Schedule")),
                ..Default::default()
            },
        ),
        Elem::field(
            AutomationTriggerField::Enabled,
            FieldOptions {
                label: Some(Label::new("Enabled")),
                ..Default::default()
            },
        ),
        Elem::field(
            AutomationTriggerField::Priority,
            FieldOptions {
                label: Some(Label::new("Priority")),
                ..Default::default()
            },
        ),
    ];
    if matches!(kind, AutomationTableKind::Consuming) {
        update_children.push(Elem::field(
            AutomationTriggerField::Mutability,
            FieldOptions {
                label: Some(Label::new("Mutability")),
                ..Default::default()
            },
        ));
        update_children.push(Elem::field(
            AutomationTriggerField::WorkItemSelector,
            FieldOptions {
                label: Some(Label::new("Work item selector")),
                ..Default::default()
            },
        ));
    }
    update_children.push(Elem::field(
        AutomationTriggerField::Prompt,
        FieldOptions {
            label: Some(Label::new("Prompt")),
            ..Default::default()
        },
    ));

    CrudInstanceConfig {
        api_base_url,
        view: SerializableCrudView::List,
        list_columns,
        create_elements: CreateElements::Custom(vec![Elem::Enclosing(Enclosing::None(Group {
            layout: Layout::default(),
            children: create_children,
        }))]),
        elements: vec![Elem::Enclosing(Enclosing::None(Group {
            layout: Layout::default(),
            children: update_children,
        }))],
        order_by: indexmap! {
            ReadAutomationTrigger::Name.into() => Order::Asc,
        },
        items_per_page: ItemsPerPage::default(),
        page_nr: PageNr::first(),
        base_condition: Some(automation_effect_condition(project_id, kind.effect())),
        resource_name: CrudAutomationTriggerResource::resource_name().to_owned(),
        reqwest_executor: Arc::new(NewClientPerRequestExecutor),
        model_handler: automation_trigger_model_handler(project_id, kind),
        actions: vec![],
        entity_actions: vec![],
        navigation: CrudNavigationConfig::default(),
        read_field_renderer: FieldRendererRegistry::builder().build(),
        create_field_renderer: FieldRendererRegistry::builder()
            .register(
                CreateAutomationTriggerField::Activation,
                activation_field_renderer::<DynCreateField>(kind.activation_choices()),
            )
            .register(
                CreateAutomationTriggerField::Prompt,
                rich_text_field_renderer::<DynCreateField>("Prompt"),
            )
            .register(
                CreateAutomationTriggerField::Mutability,
                select_field_renderer::<DynCreateField>(
                    &[("mutating", "mutating"), ("read_only", "read_only")],
                    false,
                ),
            )
            .build(),
        update_field_renderer: FieldRendererRegistry::builder()
            .register(
                AutomationTriggerField::Activation,
                activation_field_renderer::<DynUpdateField>(kind.activation_choices()),
            )
            .register(
                AutomationTriggerField::Prompt,
                rich_text_field_renderer::<DynUpdateField>("Prompt"),
            )
            .register(
                AutomationTriggerField::Mutability,
                select_field_renderer::<DynUpdateField>(
                    &[("mutating", "mutating"), ("read_only", "read_only")],
                    false,
                ),
            )
            .build(),
    }
}

fn automation_trigger_model_handler(project_id: i64, kind: AutomationTableKind) -> ModelHandler {
    let mut handler =
        ModelHandler::new::<CreateAutomationTrigger, ReadAutomationTrigger, AutomationTrigger>();
    handler.get_default_create_model = Callback::new(move |()| {
        DynCreateModel::from(CreateAutomationTrigger {
            project_id,
            activation: kind.default_activation().to_owned(),
            effect: kind.effect().to_owned(),
            work_item_selector: kind.default_selector(),
            ..Default::default()
        })
    });
    handler
}
