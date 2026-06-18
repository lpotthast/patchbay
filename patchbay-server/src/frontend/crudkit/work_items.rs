use super::*;

#[component]
pub(crate) fn WorkItemsPanel(
    api_base_url: String,
    project: String,
    project_id: i64,
) -> impl IntoView + 'static {
    view! {
        <section id="work-items-admin" class="work-items-admin panel">
            <div class="panel-heading">
                <h2>"Work items"</h2>
            </div>
            <div class="crudkit-work-items" data-crudkit-leptos="work-items">
                {work_items_crudkit_instance(api_base_url, project, project_id)}
            </div>
        </section>
    }
}

fn work_items_crudkit_instance(
    api_base_url: String,
    project: String,
    project_id: i64,
) -> impl IntoView + 'static {
    let (context, set_context) = signal(None::<CrudInstanceContext>);
    reload_crudkit_on_live_event(context, move |event| {
        event_scopes_named_project(event, Some(project.as_str()))
            && matches!(event, UiEvent::WorkItemChanged { .. })
    });

    view! {
        <CrudInstance
            name="work-items"
            config=work_items_crudkit_config(api_base_url, project_id)
            on_context_created=Callback::new(move |context| set_context.set(Some(context)))
        />
    }
}

fn work_items_crudkit_config(api_base_url: String, project_id: i64) -> CrudInstanceConfig {
    let default_create_state = Signal::derive(|| DEFAULT_STATE_LABEL.to_owned());
    let empty_label_suggestions = Signal::derive(Vec::<ProjectLabelView>::new);
    work_items_crudkit_config_for_view(
        api_base_url,
        project_id,
        SerializableCrudView::List,
        CrudNavigationConfig::default(),
        default_create_state,
        None,
        empty_label_suggestions,
    )
}

pub(crate) fn work_items_crudkit_config_for_view(
    api_base_url: String,
    project_id: i64,
    view: SerializableCrudView,
    navigation: CrudNavigationConfig,
    default_create_state: Signal<String>,
    create_state_options: Option<Signal<Vec<CreateItemStateOption>>>,
    label_suggestions: Signal<Vec<ProjectLabelView>>,
) -> CrudInstanceConfig {
    let create_elements = work_item_create_elements(create_state_options.is_some());
    let state_options_for_labels = create_state_options;
    let create_field_renderer = {
        let builder = FieldRendererRegistry::builder()
            .register(
                CreateWorkItemField::Description,
                rich_text_field_renderer::<DynCreateField>("Description"),
            )
            .register(
                CreateWorkItemField::AgentModelOverride,
                agent_model_field_renderer::<DynCreateField>(Some("Project default")),
            )
            .register(
                CreateWorkItemField::AgentReasoningEffortOverride,
                agent_reasoning_field_renderer::<DynCreateField>(Some("Project default")),
            )
            .register(
                CreateWorkItemField::InitialLabels,
                create_item_initial_labels_field_renderer::<DynCreateField>(
                    label_suggestions,
                    state_options_for_labels,
                ),
            );
        let builder = if let Some(options) = create_state_options {
            builder.register(
                CreateWorkItemField::State,
                create_item_state_field_renderer::<DynCreateField>(options),
            )
        } else {
            builder
        };
        builder.build()
    };

    CrudInstanceConfig {
        api_base_url,
        view,
        list_columns: vec![
            Header::showing(
                ReadWorkItemField::Id,
                HeaderOptions {
                    display_name: "#".into(),
                    min_width: true,
                    ..Default::default()
                },
            ),
            Header::showing(
                ReadWorkItemField::Title,
                HeaderOptions {
                    display_name: "Title".into(),
                    ..Default::default()
                },
            ),
            Header::showing(
                ReadWorkItemField::StateLabel,
                HeaderOptions {
                    display_name: "State label".into(),
                    min_width: true,
                    ..Default::default()
                },
            ),
            Header::showing(
                ReadWorkItemField::ClaimedBy,
                HeaderOptions {
                    display_name: "Claimed by".into(),
                    ..Default::default()
                },
            ),
            Header::showing(
                ReadWorkItemField::Version,
                HeaderOptions {
                    display_name: "Version".into(),
                    min_width: true,
                    ..Default::default()
                },
            ),
            Header::showing(
                ReadWorkItemField::UpdatedAt,
                HeaderOptions {
                    display_name: "Updated".into(),
                    ..Default::default()
                },
            ),
        ],
        create_elements,
        elements: vec![Elem::Enclosing(Enclosing::None(Group {
            layout: Layout::default(),
            children: vec![
                Elem::field(
                    CrudWorkItem::Id,
                    FieldOptions {
                        disabled: true,
                        label: Some(Label::new("ID")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    WorkItemField::Title,
                    FieldOptions {
                        label: Some(Label::new("Title")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    WorkItemField::Description,
                    FieldOptions {
                        label: Some(Label::new("Description")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    WorkItemField::AgentModelOverride,
                    FieldOptions {
                        label: Some(Label::new("Agent model override")),
                        ..Default::default()
                    },
                ),
                Elem::field(
                    WorkItemField::AgentReasoningEffortOverride,
                    FieldOptions {
                        label: Some(Label::new("Reasoning override")),
                        ..Default::default()
                    },
                ),
            ],
        }))],
        order_by: indexmap! {
            ReadWorkItem::Id.into() => Order::Desc,
        },
        items_per_page: ItemsPerPage::default(),
        page_nr: PageNr::first(),
        base_condition: Some(project_id_condition(project_id)),
        resource_name: CrudWorkItemResource::resource_name().to_owned(),
        reqwest_executor: Arc::new(NewClientPerRequestExecutor),
        model_handler: work_item_model_handler(project_id, default_create_state),
        actions: vec![],
        entity_actions: vec![],
        navigation,
        read_field_renderer: FieldRendererRegistry::builder()
            .register(
                ReadWorkItemField::AgentModelOverride,
                agent_model_field_renderer::<DynReadField>(Some("Project default")),
            )
            .build(),
        create_field_renderer,
        update_field_renderer: FieldRendererRegistry::builder()
            .register(
                WorkItemField::Description,
                rich_text_field_renderer::<DynUpdateField>("Description"),
            )
            .register(
                WorkItemField::AgentModelOverride,
                agent_model_field_renderer::<DynUpdateField>(Some("Project default")),
            )
            .register(
                WorkItemField::AgentReasoningEffortOverride,
                agent_reasoning_field_renderer::<DynUpdateField>(Some("Project default")),
            )
            .build(),
    }
}

fn work_item_create_elements(include_state: bool) -> CreateElements {
    let mut children = vec![
        Elem::create_field(
            CreateWorkItemField::Title,
            FieldOptions {
                label: Some(Label::new("Title")),
                ..Default::default()
            },
        ),
        Elem::create_field(
            CreateWorkItemField::Description,
            FieldOptions {
                label: Some(Label::new("Description")),
                ..Default::default()
            },
        ),
    ];
    if include_state {
        children.push(Elem::create_field(
            CreateWorkItemField::State,
            FieldOptions {
                label: Some(Label::new("State")),
                ..Default::default()
            },
        ));
    }
    children.push(Elem::create_field(
        CreateWorkItemField::InitialLabels,
        FieldOptions {
            label: Some(Label::new("Initial labels")),
            ..Default::default()
        },
    ));
    children.extend([
        Elem::create_field(
            CreateWorkItemField::AgentModelOverride,
            FieldOptions {
                label: Some(Label::new("Agent model override")),
                ..Default::default()
            },
        ),
        Elem::create_field(
            CreateWorkItemField::AgentReasoningEffortOverride,
            FieldOptions {
                label: Some(Label::new("Reasoning override")),
                ..Default::default()
            },
        ),
    ]);

    CreateElements::Custom(vec![Elem::Enclosing(Enclosing::None(Group {
        layout: Layout::default(),
        children,
    }))])
}

fn work_item_model_handler(project_id: i64, default_create_state: Signal<String>) -> ModelHandler {
    let mut handler = ModelHandler::new::<CrudCreateWorkItem, ReadWorkItem, CrudWorkItem>();
    handler.get_default_create_model = Callback::new(move |()| {
        let state = default_create_state.get_untracked();
        let state = if state.trim().is_empty() {
            DEFAULT_STATE_LABEL.to_owned()
        } else {
            state
        };
        DynCreateModel::from(CrudCreateWorkItem {
            project_id,
            state,
            initial_labels: "[]".to_owned(),
            ..Default::default()
        })
    });
    handler
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct InitialLabelRow {
    key: String,
    value: String,
}

#[derive(Clone, Copy)]
struct InitialLabelRowContext {
    key_options_id: &'static str,
    disabled: bool,
    value_signal: RwSignal<Value>,
    value_changed: Callback<Result<Value, std::sync::Arc<dyn std::error::Error>>>,
    label_suggestions: Signal<Vec<ProjectLabelView>>,
    state_options: Option<Signal<Vec<CreateItemStateOption>>>,
}

fn create_item_initial_labels_field_renderer<F: TypeErasedField>(
    label_suggestions: Signal<Vec<ProjectLabelView>>,
    state_options: Option<Signal<Vec<CreateItemStateOption>>>,
) -> FieldRenderer<F> {
    FieldRenderer::new(
        move |_signals, _field: F, field_mode, field_options, value, value_changed| {
            let rows = Signal::derive(move || initial_label_rows_from_value(&value.value.get()));

            match field_mode {
                FieldMode::Display => view! {
                    {move || initial_label_display_rows(rows.get())}
                }
                .into_any(),
                FieldMode::Readable | FieldMode::Editable => {
                    let disabled = field_mode != FieldMode::Editable || field_options.disabled;
                    let value_signal = value.value;
                    let key_options_id = "initial-label-key-options";
                    let add_row = move |_| {
                        let mut rows = initial_label_rows_from_value(&value_signal.get_untracked());
                        rows.push(InitialLabelRow::default());
                        value_changed.run(Ok(initial_label_rows_to_value(&rows)));
                    };
                    let label_suggestions_for_keys = label_suggestions;
                    let row_context = InitialLabelRowContext {
                        key_options_id,
                        disabled,
                        value_signal,
                        value_changed,
                        label_suggestions,
                        state_options,
                    };
                    view! {
                        {render_label(field_options.label.clone())}
                        <div class="initial-labels-field" data-initial-labels-field="create">
                            <datalist id=key_options_id>
                                {move || initial_label_key_options(label_suggestions_for_keys.get())}
                            </datalist>
                            <div class="initial-label-rows">
                                {move || {
                                    rows.get()
                                        .into_iter()
                                        .enumerate()
                                        .map(|(index, row)| {
                                            initial_label_row_view(index, row, row_context)
                                        })
                                        .collect::<Vec<_>>()
                                }}
                            </div>
                            <button
                                type="button"
                                class="secondary initial-label-add"
                                disabled=disabled
                                on:click=add_row
                            >
                                <Icon icon=icondata::BsPlusLg/>
                                <span>"Add label"</span>
                            </button>
                        </div>
                    }
                    .into_any()
                }
            }
        },
    )
}

fn initial_label_row_view(
    index: usize,
    row: InitialLabelRow,
    context: InitialLabelRowContext,
) -> impl IntoView {
    let value_options_id = format!("initial-label-value-options-{index}");
    let value_options_id_for_input = value_options_id.clone();
    let row_key_for_options = row.key.clone();
    let row_key = row.key;
    let row_value = row.value;
    let update_key = move |event| {
        let mut rows = initial_label_rows_from_value(&context.value_signal.get_untracked());
        if let Some(row) = rows.get_mut(index) {
            row.key = event_target_value(&event);
        }
        context
            .value_changed
            .run(Ok(initial_label_rows_to_value(&rows)));
    };
    let update_value = move |event| {
        let mut rows = initial_label_rows_from_value(&context.value_signal.get_untracked());
        if let Some(row) = rows.get_mut(index) {
            row.value = event_target_value(&event);
        }
        context
            .value_changed
            .run(Ok(initial_label_rows_to_value(&rows)));
    };
    let remove_row = move |_| {
        let mut rows = initial_label_rows_from_value(&context.value_signal.get_untracked());
        if index < rows.len() {
            rows.remove(index);
        }
        context
            .value_changed
            .run(Ok(initial_label_rows_to_value(&rows)));
    };

    view! {
        <div class="initial-label-row">
            <datalist id=value_options_id>
                {move || {
                    let state_options = context
                        .state_options
                        .map(|options| options.get())
                        .unwrap_or_default();
                    initial_label_value_options(
                        context.label_suggestions.get(),
                        state_options,
                        row_key_for_options.clone(),
                    )
                }}
            </datalist>
            <input
                type="text"
                class="crud-input-field initial-label-key"
                name="initial_label_key"
                list=context.key_options_id
                prop:value=row_key
                placeholder="type"
                disabled=context.disabled
                on:input=update_key
            />
            <input
                type="text"
                class="crud-input-field initial-label-value"
                name="initial_label_value"
                list=value_options_id_for_input
                prop:value=row_value
                placeholder="feature"
                disabled=context.disabled
                on:input=update_value
            />
            <button
                type="button"
                class="secondary icon-button initial-label-remove"
                title="Remove label"
                aria-label="Remove label"
                disabled=context.disabled
                on:click=remove_row
            >
                <Icon icon=icondata::BsTrash/>
            </button>
        </div>
    }
}

fn initial_label_rows_from_value(value: &Value) -> Vec<InitialLabelRow> {
    let json = match value {
        Value::String(raw) => serde_json::from_str::<serde_json::Value>(raw).ok(),
        Value::Json(json) => Some(json.clone()),
        _ => None,
    };
    json.as_ref()
        .and_then(|value| value.as_array())
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    let object = row.as_object()?;
                    let key = object.get("key").and_then(|value| value.as_str())?;
                    let value = object
                        .get("value")
                        .and_then(|value| value.as_str())
                        .unwrap_or_default();
                    Some(InitialLabelRow {
                        key: key.to_owned(),
                        value: value.to_owned(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn initial_label_rows_to_value(rows: &[InitialLabelRow]) -> Value {
    let json = serde_json::Value::Array(
        rows.iter()
            .map(|row| {
                serde_json::json!({
                    "key": row.key,
                    "value": if row.value.trim().is_empty() {
                        serde_json::Value::Null
                    } else {
                        serde_json::Value::String(row.value.clone())
                    },
                })
            })
            .collect(),
    );
    Value::String(serde_json::to_string(&json).unwrap_or_else(|_| "[]".to_owned()))
}

fn initial_label_display_rows(rows: Vec<InitialLabelRow>) -> Vec<AnyView> {
    rows.into_iter()
        .map(|row| {
            let label = if row.value.trim().is_empty() {
                row.key
            } else {
                format!("{}={}", row.key, row.value)
            };
            view! { <span class="label-pill">{label}</span> }.into_any()
        })
        .collect()
}

fn initial_label_key_options(suggestions: Vec<ProjectLabelView>) -> Vec<AnyView> {
    let mut keys = std::collections::BTreeSet::new();
    suggestions
        .into_iter()
        .filter_map(|suggestion| {
            (suggestion.key != STATE_LABEL_KEY && keys.insert(suggestion.key.clone()))
                .then_some(suggestion.key)
        })
        .map(|key| view! { <option value=key></option> }.into_any())
        .collect()
}

fn initial_label_value_options(
    suggestions: Vec<ProjectLabelView>,
    state_options: Vec<CreateItemStateOption>,
    key: String,
) -> Vec<AnyView> {
    let mut values = std::collections::BTreeSet::new();
    if key.trim() == STATE_LABEL_KEY {
        return state_options
            .into_iter()
            .filter_map(|option| values.insert(option.identifier.clone()).then_some(option))
            .map(|option| {
                view! { <option value=option.identifier>{option.name}</option> }.into_any()
            })
            .collect();
    }

    suggestions
        .into_iter()
        .filter(|suggestion| suggestion.key == key)
        .filter_map(|suggestion| suggestion.value)
        .filter_map(|value| values.insert(value.clone()).then_some(value))
        .map(|value| view! { <option value=value></option> }.into_any())
        .collect()
}

fn create_item_state_option_views(
    options: Vec<CreateItemStateOption>,
    selected_state: String,
) -> Vec<AnyView> {
    if options.is_empty() {
        return vec![
            view! {
                <option value="" selected=true>"No states available"</option>
            }
            .into_any(),
        ];
    }

    options
        .into_iter()
        .map(|option| {
            let selected = option.identifier == selected_state;
            view! {
                <option value=option.identifier selected=selected>
                    {option.name}
                </option>
            }
            .into_any()
        })
        .collect()
}

fn create_item_state_field_renderer<F: TypeErasedField>(
    options: Signal<Vec<CreateItemStateOption>>,
) -> FieldRenderer<F> {
    FieldRenderer::new(
        move |_signals, _field: F, field_mode, field_options, value, value_changed| {
            let current =
                Signal::derive(move || value.value.get().as_string().cloned().unwrap_or_default());

            match field_mode {
                FieldMode::Display => view! {
                    {move || {
                        let current = current.get();
                        options
                            .get()
                            .into_iter()
                            .find(|option| option.identifier == current)
                            .map(|option| option.name)
                            .unwrap_or(current)
                    }}
                }
                .into_any(),
                FieldMode::Readable | FieldMode::Editable => {
                    let disabled = field_mode != FieldMode::Editable || field_options.disabled;
                    view! {
                        {render_label(field_options.label.clone())}
                        <select
                            name="state"
                            class="crud-input-field work-item-state-select"
                            prop:value=move || current.get()
                            disabled=move || disabled || options.get().is_empty()
                            on:change=move |event| {
                                value_changed.run(Ok(Value::String(event_target_value(&event))));
                            }
                        >
                            {move || create_item_state_option_views(options.get(), current.get())}
                        </select>
                    }
                    .into_any()
                }
            }
        },
    )
}
