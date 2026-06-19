use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FilterGroupKind {
    All,
    Any,
}

impl FilterGroupKind {
    fn from_storage(value: &str) -> Self {
        match value {
            "any" => Self::Any,
            _ => Self::All,
        }
    }

    fn as_storage(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Any => "any",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Any => "Any",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FilterGroupDraft {
    kind: FilterGroupKind,
    elements: Vec<FilterElementDraft>,
}

impl FilterGroupDraft {
    fn match_all() -> Self {
        Self {
            kind: FilterGroupKind::All,
            elements: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum FilterElementDraft {
    Group(FilterGroupDraft),
    Clause(FilterClauseDraft),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FilterClauseDraft {
    key: String,
    mode: FilterClauseMode,
}

impl FilterClauseDraft {
    fn empty() -> Self {
        Self {
            key: String::new(),
            mode: FilterClauseMode::Equals {
                value: String::new(),
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum FilterClauseMode {
    Equals { value: String },
    NotEquals { value: String },
    Present,
    Absent,
    HasNoValue,
    HasValueOrAbsent,
    IsIn { values: Vec<String> },
}

impl FilterClauseMode {
    fn from_storage(value: &str, current: &Self) -> Self {
        match value {
            "not_equals" => Self::NotEquals {
                value: current.primary_value(),
            },
            "present" => Self::Present,
            "absent" => Self::Absent,
            "no_value" => Self::HasNoValue,
            "has_value_or_absent" => Self::HasValueOrAbsent,
            "is_in" => Self::IsIn {
                values: current.list_values(),
            },
            _ => Self::Equals {
                value: current.primary_value(),
            },
        }
    }

    fn as_storage(&self) -> &'static str {
        match self {
            Self::Equals { .. } => "equals",
            Self::NotEquals { .. } => "not_equals",
            Self::Present => "present",
            Self::Absent => "absent",
            Self::HasNoValue => "no_value",
            Self::HasValueOrAbsent => "has_value_or_absent",
            Self::IsIn { .. } => "is_in",
        }
    }

    fn primary_value(&self) -> String {
        match self {
            Self::Equals { value } | Self::NotEquals { value } => value.clone(),
            Self::IsIn { values } => values.first().cloned().unwrap_or_default(),
            Self::Present | Self::Absent | Self::HasNoValue | Self::HasValueOrAbsent => {
                String::new()
            }
        }
    }

    fn list_values(&self) -> Vec<String> {
        match self {
            Self::IsIn { values } => values.clone(),
            Self::Equals { value } | Self::NotEquals { value } if !value.trim().is_empty() => {
                vec![value.clone()]
            }
            _ => Vec::new(),
        }
    }
}

#[derive(Clone, Copy)]
struct FilterEditorContext {
    disabled: bool,
    value_signal: RwSignal<Value>,
    value_changed: Callback<Result<Value, std::sync::Arc<dyn std::error::Error>>>,
}

pub(super) fn swim_lane_filter_field_renderer<F: TypeErasedField>() -> FieldRenderer<F> {
    FieldRenderer::new(
        move |_signals, _field: F, field_mode, field_options, value, value_changed| {
            let current = Signal::derive(move || filter_string_from_value(&value.value.get()));

            match field_mode {
                FieldMode::Display => {
                    view! { <span>{move || filter_display_summary(&current.get())}</span> }
                        .into_any()
                }
                FieldMode::Readable | FieldMode::Editable => {
                    let disabled = field_mode != FieldMode::Editable || field_options.disabled;
                    let raw_mode = RwSignal::new(false);
                    let context = FilterEditorContext {
                        disabled,
                        value_signal: value.value,
                        value_changed,
                    };

                    view! {
                        {render_label(field_options.label.clone())}
                        <div class="swim-lane-filter-editor" data-lane-filter-editor="structured">
                            {move || {
                                let raw = current.get();
                                let parsed = parse_filter_draft(&raw);
                                match parsed {
                                    Ok(draft) if !raw_mode.get() => structured_filter_view(draft, raw_mode, context),
                                    Ok(_) => raw_filter_view(raw, None, raw_mode, context),
                                    Err(error) => raw_filter_view(raw, Some(error), raw_mode, context),
                                }
                            }}
                        </div>
                    }
                    .into_any()
                }
            }
        },
    )
}

fn structured_filter_view(
    draft: FilterGroupDraft,
    raw_mode: RwSignal<bool>,
    context: FilterEditorContext,
) -> AnyView {
    view! {
        <div class="lane-filter-structured">
            {filter_group_view(draft, Vec::new(), context)}
            <div class="lane-filter-footer">
                <button
                    type="button"
                    class="secondary lane-filter-raw-toggle"
                    disabled=context.disabled
                    on:click=move |_| raw_mode.set(true)
                >
                    "Edit raw JSON"
                </button>
            </div>
        </div>
    }
    .into_any()
}

fn raw_filter_view(
    raw: String,
    error: Option<String>,
    raw_mode: RwSignal<bool>,
    context: FilterEditorContext,
) -> AnyView {
    let can_use_structured = error.is_none() && parse_filter_draft(&raw).is_ok();
    let current_raw = raw.clone();
    let error_view = error.map(|error| {
        view! {
            <p class="lane-filter-error" role="alert">
                {"Cannot show this filter as structured controls: "}
                {error}
            </p>
        }
    });

    view! {
        <div class="lane-filter-raw-panel">
            {error_view}
            <textarea
                class="crud-input-field lane-filter-raw"
                data-lane-filter-raw="true"
                prop:value=current_raw
                disabled=context.disabled
                on:input=move |event| {
                    raw_mode.set(true);
                    context
                        .value_changed
                        .run(Ok(Value::String(event_target_value(&event))));
                }
            />
            <div class="lane-filter-footer">
                <button
                    type="button"
                    class="secondary lane-filter-structured-toggle"
                    disabled=move || context.disabled || !can_use_structured
                    on:click=move |_| raw_mode.set(false)
                >
                    "Use structured editor"
                </button>
            </div>
        </div>
    }
    .into_any()
}

fn filter_group_view(
    group: FilterGroupDraft,
    path: Vec<usize>,
    context: FilterEditorContext,
) -> AnyView {
    let group_label = group.kind.label();
    let group_kind = group.kind.as_storage().to_owned();
    let path_attr = filter_path_attr(&path);
    let path_for_kind = path.clone();
    let change_kind = move |event| {
        let selected = event_target_value(&event);
        update_filter_draft(context, |draft| {
            if let Some(group) = filter_group_mut(draft, &path_for_kind) {
                group.kind = FilterGroupKind::from_storage(&selected);
            }
        });
    };

    let path_for_clause = path.clone();
    let add_clause = move |_| {
        update_filter_draft(context, |draft| {
            if let Some(group) = filter_group_mut(draft, &path_for_clause) {
                group
                    .elements
                    .push(FilterElementDraft::Clause(FilterClauseDraft::empty()));
            }
        });
    };

    let path_for_group = path.clone();
    let add_group = move |_| {
        update_filter_draft(context, |draft| {
            if let Some(group) = filter_group_mut(draft, &path_for_group) {
                group
                    .elements
                    .push(FilterElementDraft::Group(FilterGroupDraft::match_all()));
            }
        });
    };

    let remove_group_button = (!path.is_empty()).then(|| {
        let parent_path = path[..path.len() - 1].to_vec();
        let index = *path.last().unwrap_or(&0);
        view! {
            <button
                type="button"
                class="secondary icon-button lane-filter-remove-group"
                title="Remove group"
                aria-label="Remove group"
                disabled=context.disabled
                on:click=move |_| remove_filter_element(context, &parent_path, index)
            >
                <Icon icon=icondata::BsTrash/>
            </button>
        }
    });

    let empty_group = group.elements.is_empty().then(|| {
        view! {
            <p class="lane-filter-empty">
                "No rules in this group. It matches all work items."
            </p>
        }
    });

    let elements = group
        .elements
        .into_iter()
        .enumerate()
        .map(|(index, element)| filter_element_view(element, path.clone(), index, context))
        .collect::<Vec<_>>();

    view! {
        <div class="lane-filter-group" data-lane-filter-group=path_attr>
            <div class="lane-filter-group-header">
                <label class="lane-filter-group-kind">
                    <span>"Match"</span>
                    <select
                        class="crud-input-field"
                        aria-label="Group match mode"
                        prop:value=group_kind
                        disabled=context.disabled
                        on:change=change_kind
                    >
                        <option value="all">"All"</option>
                        <option value="any">"Any"</option>
                    </select>
                    <span>{group_label}" rules"</span>
                </label>
                <div class="lane-filter-actions">
                    <button
                        type="button"
                        class="secondary lane-filter-add-clause"
                        data-lane-filter-add-clause="true"
                        disabled=context.disabled
                        on:click=add_clause
                    >
                        <Icon icon=icondata::BsPlusLg/>
                        <span>"Add label"</span>
                    </button>
                    <button
                        type="button"
                        class="secondary lane-filter-add-group"
                        data-lane-filter-add-group="true"
                        disabled=context.disabled
                        on:click=add_group
                    >
                        <Icon icon=icondata::BsPlusLg/>
                        <span>"Add group"</span>
                    </button>
                    {remove_group_button}
                </div>
            </div>
            <div class="lane-filter-elements">
                {empty_group}
                {elements}
            </div>
        </div>
    }
    .into_any()
}

fn filter_element_view(
    element: FilterElementDraft,
    parent_path: Vec<usize>,
    index: usize,
    context: FilterEditorContext,
) -> AnyView {
    match element {
        FilterElementDraft::Clause(clause) => {
            filter_clause_view(clause, parent_path, index, context)
        }
        FilterElementDraft::Group(group) => {
            let mut child_path = parent_path;
            child_path.push(index);
            filter_group_view(group, child_path, context)
        }
    }
}

fn filter_clause_view(
    clause: FilterClauseDraft,
    parent_path: Vec<usize>,
    index: usize,
    context: FilterEditorContext,
) -> AnyView {
    let key = clause.key.clone();
    let mode = clause.mode.clone();
    let mode_value = mode.as_storage().to_owned();

    let path_for_key = parent_path.clone();
    let update_key = move |event| {
        let next_key = event_target_value(&event);
        update_filter_draft(context, |draft| {
            if let Some(clause) = filter_clause_mut(draft, &path_for_key, index) {
                clause.key = next_key;
            }
        });
    };

    let path_for_mode = parent_path.clone();
    let update_mode = move |event| {
        let selected = event_target_value(&event);
        update_filter_draft(context, |draft| {
            if let Some(clause) = filter_clause_mut(draft, &path_for_mode, index) {
                clause.mode = FilterClauseMode::from_storage(&selected, &clause.mode);
            }
        });
    };

    let value_control = match mode {
        FilterClauseMode::Equals { value } | FilterClauseMode::NotEquals { value } => {
            let path_for_value = parent_path.clone();
            view! {
                <input
                    type="text"
                    class="crud-input-field lane-filter-value"
                    data-lane-filter-value="true"
                    aria-label="Label value"
                    prop:value=value
                    placeholder="value"
                    disabled=context.disabled
                    on:input=move |event| {
                        let next_value = event_target_value(&event);
                        update_filter_draft(context, |draft| {
                            if let Some(clause) = filter_clause_mut(draft, &path_for_value, index) {
                                match &mut clause.mode {
                                    FilterClauseMode::Equals { value }
                                    | FilterClauseMode::NotEquals { value } => *value = next_value,
                                    FilterClauseMode::Present
                                    | FilterClauseMode::Absent
                                    | FilterClauseMode::HasNoValue
                                    | FilterClauseMode::HasValueOrAbsent
                                    | FilterClauseMode::IsIn { .. } => {}
                                }
                            }
                        });
                    }
                />
            }
            .into_any()
        }
        FilterClauseMode::IsIn { values } => {
            let path_for_values = parent_path.clone();
            view! {
                <input
                    type="text"
                    class="crud-input-field lane-filter-value-list"
                    data-lane-filter-value-list="true"
                    aria-label="Label value list"
                    prop:value=join_list_values(&values)
                    placeholder="value list"
                    disabled=context.disabled
                    on:input=move |event| {
                        let next_values = split_list_values(&event_target_value(&event));
                        update_filter_draft(context, |draft| {
                            if let Some(clause) = filter_clause_mut(draft, &path_for_values, index)
                                && let FilterClauseMode::IsIn { values } = &mut clause.mode
                            {
                                *values = next_values;
                            }
                        });
                    }
                />
            }
            .into_any()
        }
        FilterClauseMode::Present
        | FilterClauseMode::Absent
        | FilterClauseMode::HasNoValue
        | FilterClauseMode::HasValueOrAbsent => {
            view! { <span class="lane-filter-no-value">"No value needed"</span> }.into_any()
        }
    };

    let path_for_remove = parent_path.clone();
    let remove_clause = move |_| remove_filter_element(context, &path_for_remove, index);

    view! {
        <div class="lane-filter-clause" data-lane-filter-clause="true">
            <input
                type="text"
                class="crud-input-field lane-filter-key"
                data-lane-filter-key="true"
                aria-label="Label key"
                prop:value=key
                placeholder="label key"
                disabled=context.disabled
                on:input=update_key
            />
            <select
                class="crud-input-field lane-filter-operator"
                data-lane-filter-operator="true"
                aria-label="Label operator"
                prop:value=mode_value
                disabled=context.disabled
                on:change=update_mode
            >
                <option value="equals">"is"</option>
                <option value="not_equals">"is not"</option>
                <option value="present">"label present"</option>
                <option value="absent">"label absent"</option>
                <option value="no_value">"has no value"</option>
                <option value="has_value_or_absent">"has a value or is absent"</option>
                <option value="is_in">"is any of"</option>
            </select>
            {value_control}
            <button
                type="button"
                class="secondary icon-button lane-filter-remove-clause"
                title="Remove label condition"
                aria-label="Remove label condition"
                disabled=context.disabled
                on:click=remove_clause
            >
                <Icon icon=icondata::BsTrash/>
            </button>
        </div>
    }
    .into_any()
}

fn update_filter_draft(context: FilterEditorContext, update: impl FnOnce(&mut FilterGroupDraft)) {
    let raw = filter_string_from_value(&context.value_signal.get_untracked());
    let mut draft = parse_filter_draft(&raw).unwrap_or_else(|_| FilterGroupDraft::match_all());
    update(&mut draft);
    match serialize_filter_draft(&draft) {
        Ok(serialized) => context.value_changed.run(Ok(Value::String(serialized))),
        Err(error) => context
            .value_changed
            .run(Err(std::sync::Arc::new(FilterEditorError(error)))),
    }
}

fn remove_filter_element(context: FilterEditorContext, parent_path: &[usize], index: usize) {
    update_filter_draft(context, |draft| {
        if let Some(group) = filter_group_mut(draft, parent_path)
            && index < group.elements.len()
        {
            group.elements.remove(index);
        }
    });
}

fn filter_group_mut<'a>(
    group: &'a mut FilterGroupDraft,
    path: &[usize],
) -> Option<&'a mut FilterGroupDraft> {
    let mut current = group;
    for index in path {
        match current.elements.get_mut(*index) {
            Some(FilterElementDraft::Group(group)) => current = group,
            Some(FilterElementDraft::Clause(_)) | None => return None,
        }
    }
    Some(current)
}

fn filter_clause_mut<'a>(
    group: &'a mut FilterGroupDraft,
    parent_path: &[usize],
    index: usize,
) -> Option<&'a mut FilterClauseDraft> {
    let group = filter_group_mut(group, parent_path)?;
    match group.elements.get_mut(index) {
        Some(FilterElementDraft::Clause(clause)) => Some(clause),
        Some(FilterElementDraft::Group(_)) | None => None,
    }
}

fn parse_filter_draft(raw: &str) -> Result<FilterGroupDraft, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(FilterGroupDraft::match_all());
    }
    let condition = serde_json::from_str::<Condition>(raw)
        .map_err(|err| format!("expected CrudKit Condition JSON ({err})"))?;
    condition_to_draft(&condition)
}

fn serialize_filter_draft(draft: &FilterGroupDraft) -> Result<String, String> {
    serde_json::to_string(&draft_to_condition(draft))
        .map_err(|err| format!("failed to serialize filter ({err})"))
}

fn condition_to_draft(condition: &Condition) -> Result<FilterGroupDraft, String> {
    let (kind, elements) = match condition {
        Condition::All(elements) => (FilterGroupKind::All, elements),
        Condition::Any(elements) => (FilterGroupKind::Any, elements),
    };
    let elements = elements
        .iter()
        .map(|element| match element {
            ConditionElement::Clause(clause) => {
                clause_to_draft(clause).map(FilterElementDraft::Clause)
            }
            ConditionElement::Condition(condition) => {
                condition_to_draft(condition).map(FilterElementDraft::Group)
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(FilterGroupDraft { kind, elements })
}

fn clause_to_draft(clause: &ConditionClause) -> Result<FilterClauseDraft, String> {
    let mode = match (&clause.operator, &clause.value) {
        (Operator::Equal, ConditionClauseValue::String(value)) => FilterClauseMode::Equals {
            value: value.clone(),
        },
        (Operator::NotEqual, ConditionClauseValue::String(value)) => FilterClauseMode::NotEquals {
            value: value.clone(),
        },
        (Operator::Equal, ConditionClauseValue::Bool(true))
        | (Operator::NotEqual, ConditionClauseValue::Bool(false)) => FilterClauseMode::Present,
        (Operator::Equal, ConditionClauseValue::Bool(false))
        | (Operator::NotEqual, ConditionClauseValue::Bool(true)) => FilterClauseMode::Absent,
        (Operator::Equal, ConditionClauseValue::Json(serde_json::Value::Null)) => {
            FilterClauseMode::HasNoValue
        }
        (Operator::NotEqual, ConditionClauseValue::Json(serde_json::Value::Null)) => {
            FilterClauseMode::HasValueOrAbsent
        }
        (Operator::IsIn, ConditionClauseValue::Json(serde_json::Value::Array(values))) => {
            let values = values
                .iter()
                .map(|value| {
                    value.as_str().map(ToOwned::to_owned).ok_or_else(|| {
                        format!(
                            "label '{}' uses is_in with a non-string list value",
                            clause.column_name
                        )
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            FilterClauseMode::IsIn { values }
        }
        (Operator::IsIn, _) => {
            return Err(format!(
                "label '{}' uses is_in without a string list",
                clause.column_name
            ));
        }
        (operator, _) => {
            return Err(format!(
                "label '{}' uses unsupported operator '{}'",
                clause.column_name,
                operator_label(*operator)
            ));
        }
    };
    Ok(FilterClauseDraft {
        key: clause.column_name.clone(),
        mode,
    })
}

fn draft_to_condition(draft: &FilterGroupDraft) -> Condition {
    let elements = draft
        .elements
        .iter()
        .map(|element| match element {
            FilterElementDraft::Clause(clause) => {
                ConditionElement::Clause(draft_clause_to_condition(clause))
            }
            FilterElementDraft::Group(group) => {
                ConditionElement::Condition(Box::new(draft_to_condition(group)))
            }
        })
        .collect::<Vec<_>>();
    match draft.kind {
        FilterGroupKind::All => Condition::All(elements),
        FilterGroupKind::Any => Condition::Any(elements),
    }
}

fn draft_clause_to_condition(clause: &FilterClauseDraft) -> ConditionClause {
    let (operator, value) = match &clause.mode {
        FilterClauseMode::Equals { value } => {
            (Operator::Equal, ConditionClauseValue::String(value.clone()))
        }
        FilterClauseMode::NotEquals { value } => (
            Operator::NotEqual,
            ConditionClauseValue::String(value.clone()),
        ),
        FilterClauseMode::Present => (Operator::Equal, ConditionClauseValue::Bool(true)),
        FilterClauseMode::Absent => (Operator::Equal, ConditionClauseValue::Bool(false)),
        FilterClauseMode::HasNoValue => (
            Operator::Equal,
            ConditionClauseValue::Json(serde_json::Value::Null),
        ),
        FilterClauseMode::HasValueOrAbsent => (
            Operator::NotEqual,
            ConditionClauseValue::Json(serde_json::Value::Null),
        ),
        FilterClauseMode::IsIn { values } => (
            Operator::IsIn,
            ConditionClauseValue::Json(serde_json::Value::Array(
                values
                    .iter()
                    .map(|value| serde_json::Value::String(value.clone()))
                    .collect(),
            )),
        ),
    };
    ConditionClause {
        column_name: clause.key.clone(),
        operator,
        value,
    }
}

fn filter_display_summary(raw: &str) -> String {
    parse_filter_draft(raw)
        .map(|draft| group_summary(&draft))
        .unwrap_or_else(|_| raw.to_owned())
}

fn group_summary(group: &FilterGroupDraft) -> String {
    if group.elements.is_empty() {
        return "Match all".to_owned();
    }
    let joiner = match group.kind {
        FilterGroupKind::All => " and ",
        FilterGroupKind::Any => " or ",
    };
    group
        .elements
        .iter()
        .map(|element| match element {
            FilterElementDraft::Clause(clause) => clause_summary(clause),
            FilterElementDraft::Group(group) => format!("({})", group_summary(group)),
        })
        .collect::<Vec<_>>()
        .join(joiner)
}

fn clause_summary(clause: &FilterClauseDraft) -> String {
    match &clause.mode {
        FilterClauseMode::Equals { value } => format!("{} is {}", clause.key, value),
        FilterClauseMode::NotEquals { value } => format!("{} is not {}", clause.key, value),
        FilterClauseMode::Present => format!("{} present", clause.key),
        FilterClauseMode::Absent => format!("{} absent", clause.key),
        FilterClauseMode::HasNoValue => format!("{} has no value", clause.key),
        FilterClauseMode::HasValueOrAbsent => format!("{} has a value or is absent", clause.key),
        FilterClauseMode::IsIn { values } => {
            format!("{} is any of {}", clause.key, join_list_values(values))
        }
    }
}

fn filter_string_from_value(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Json(value) => value.to_string(),
        Value::Null | Value::Void(()) => String::new(),
        other => format!("{other:?}"),
    }
}

fn split_list_values(value: &str) -> Vec<String> {
    value
        .split([',', '\n'])
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn join_list_values(values: &[String]) -> String {
    values.join(", ")
}

fn filter_path_attr(path: &[usize]) -> String {
    if path.is_empty() {
        "root".to_owned()
    } else {
        path.iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(".")
    }
}

fn operator_label(operator: Operator) -> &'static str {
    match operator {
        Operator::Equal => "=",
        Operator::NotEqual => "!=",
        Operator::Less => "<",
        Operator::LessOrEqual => "<=",
        Operator::Greater => ">",
        Operator::GreaterOrEqual => ">=",
        Operator::IsIn => "is_in",
    }
}

#[derive(Debug)]
struct FilterEditorError(String);

impl std::fmt::Display for FilterEditorError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for FilterEditorError {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn clause(key: &str, mode: FilterClauseMode) -> FilterElementDraft {
        FilterElementDraft::Clause(FilterClauseDraft {
            key: key.to_owned(),
            mode,
        })
    }

    fn parsed(raw: &str) -> FilterGroupDraft {
        parse_filter_draft(raw).unwrap()
    }

    fn serialized_condition(draft: &FilterGroupDraft) -> Condition {
        serde_json::from_str(&serialize_filter_draft(draft).unwrap()).unwrap()
    }

    #[test]
    fn empty_filter_parses_as_match_all() {
        assert_eq!(
            parse_filter_draft("").unwrap(),
            FilterGroupDraft::match_all()
        );
        assert_eq!(
            serialized_condition(&FilterGroupDraft::match_all()),
            Condition::All(Vec::new())
        );
    }

    #[test]
    fn nested_groups_round_trip() {
        let raw = r#"{"All":[{"column_name":"state","operator":"=","value":{"String":"open"}},{"Any":[{"column_name":"priority","operator":"=","value":{"String":"high"}},{"column_name":"needs-verification","operator":"=","value":{"Bool":true}}]}]}"#;
        let draft = parsed(raw);

        assert_eq!(
            draft,
            FilterGroupDraft {
                kind: FilterGroupKind::All,
                elements: vec![
                    clause(
                        "state",
                        FilterClauseMode::Equals {
                            value: "open".to_owned()
                        }
                    ),
                    FilterElementDraft::Group(FilterGroupDraft {
                        kind: FilterGroupKind::Any,
                        elements: vec![
                            clause(
                                "priority",
                                FilterClauseMode::Equals {
                                    value: "high".to_owned()
                                }
                            ),
                            clause("needs-verification", FilterClauseMode::Present),
                        ],
                    }),
                ],
            }
        );

        assert_eq!(
            serialized_condition(&draft),
            serde_json::from_str::<Condition>(raw).unwrap()
        );
    }

    #[test]
    fn supported_clause_value_modes_round_trip() {
        let draft = FilterGroupDraft {
            kind: FilterGroupKind::All,
            elements: vec![
                clause(
                    "state",
                    FilterClauseMode::Equals {
                        value: "open".to_owned(),
                    },
                ),
                clause(
                    "priority",
                    FilterClauseMode::NotEquals {
                        value: "low".to_owned(),
                    },
                ),
                clause("needs-verification", FilterClauseMode::Present),
                clause("patchbay:automation-blocked", FilterClauseMode::Absent),
                clause("flag", FilterClauseMode::HasNoValue),
                clause("valued", FilterClauseMode::HasValueOrAbsent),
                clause(
                    "severity",
                    FilterClauseMode::IsIn {
                        values: vec!["critical".to_owned(), "high".to_owned()],
                    },
                ),
            ],
        };

        let condition = serialized_condition(&draft);
        assert_eq!(
            condition,
            Condition::All(vec![
                ConditionElement::Clause(ConditionClause {
                    column_name: "state".to_owned(),
                    operator: Operator::Equal,
                    value: ConditionClauseValue::String("open".to_owned()),
                }),
                ConditionElement::Clause(ConditionClause {
                    column_name: "priority".to_owned(),
                    operator: Operator::NotEqual,
                    value: ConditionClauseValue::String("low".to_owned()),
                }),
                ConditionElement::Clause(ConditionClause {
                    column_name: "needs-verification".to_owned(),
                    operator: Operator::Equal,
                    value: ConditionClauseValue::Bool(true),
                }),
                ConditionElement::Clause(ConditionClause {
                    column_name: "patchbay:automation-blocked".to_owned(),
                    operator: Operator::Equal,
                    value: ConditionClauseValue::Bool(false),
                }),
                ConditionElement::Clause(ConditionClause {
                    column_name: "flag".to_owned(),
                    operator: Operator::Equal,
                    value: ConditionClauseValue::Json(serde_json::Value::Null),
                }),
                ConditionElement::Clause(ConditionClause {
                    column_name: "valued".to_owned(),
                    operator: Operator::NotEqual,
                    value: ConditionClauseValue::Json(serde_json::Value::Null),
                }),
                ConditionElement::Clause(ConditionClause {
                    column_name: "severity".to_owned(),
                    operator: Operator::IsIn,
                    value: ConditionClauseValue::Json(json!(["critical", "high"])),
                }),
            ])
        );
        assert_eq!(parsed(&serialize_filter_draft(&draft).unwrap()), draft);
    }

    #[test]
    fn bool_not_equal_parses_to_equivalent_presence_modes() {
        let raw = r#"{"All":[{"column_name":"ready","operator":"!=","value":{"Bool":false}},{"column_name":"blocked","operator":"!=","value":{"Bool":true}}]}"#;

        assert_eq!(
            parsed(raw),
            FilterGroupDraft {
                kind: FilterGroupKind::All,
                elements: vec![
                    clause("ready", FilterClauseMode::Present),
                    clause("blocked", FilterClauseMode::Absent),
                ],
            }
        );
    }

    #[test]
    fn unsupported_condition_reports_actionable_error() {
        let error = parse_filter_draft(
            r#"{"All":[{"column_name":"state","operator":">","value":{"String":"open"}}]}"#,
        )
        .unwrap_err();

        assert!(error.contains("unsupported operator '>'"));
    }

    #[test]
    fn list_values_split_on_commas_and_newlines() {
        assert_eq!(
            split_list_values("critical, high\nlow,, "),
            vec!["critical", "high", "low"]
        );
    }
}
