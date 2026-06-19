use crudkit_core::condition::{
    Condition, ConditionClause, ConditionClauseValue, ConditionElement, Operator,
};
use sea_orm::{ActiveModelTrait, ActiveValue::Set};
use tempfile::TempDir;
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

use super::*;
use crate::backend::{
    agent_ids,
    comments::list_comments,
    entities::{agent_run, work_item::WorkItemActiveModel},
    item_label_service::add_label,
    items::{CreateWorkItem, create_item, get_item, list_events, list_items, move_item},
    projects::{self, CreateProject, create_project},
    storage::{Store, utc_now},
    work_items,
};
use crate::shared::view_models::{
    AUTOMATION_BLOCKED_LABEL_KEY, AuthorType, CLAIMED_FROM_STATE_LABEL_KEY, CLAIMED_STATE_LABEL,
    FEEDBACK_REQUESTED_LABEL_KEY, FINISHED_STATE_LABEL, STATE_LABEL_KEY,
};

async fn test_store() -> (TempDir, Store) {
    let temp = TempDir::new().unwrap();
    let store = Store::open(temp.path().join("patchbay.sqlite3"))
        .await
        .unwrap();
    create_project(
        &store,
        CreateProject {
            name: "demo".to_owned(),
            display_name: None,
            path: temp.path().to_path_buf(),
            default_agent_model: None,
            default_agent_reasoning_effort: None,
            system_prompt: None,
            memory: None,
        },
    )
    .await
    .unwrap();
    create_project(
        &store,
        CreateProject {
            name: "other".to_owned(),
            display_name: None,
            path: temp.path().to_path_buf(),
            default_agent_model: None,
            default_agent_reasoning_effort: None,
            system_prompt: None,
            memory: None,
        },
    )
    .await
    .unwrap();
    (temp, store)
}

#[tokio::test]
async fn claiming_item_records_agent_identity() {
    let (_temp, store) = test_store().await;
    let item = create_item(
        &store,
        "demo",
        CreateWorkItem {
            title: "Claim me".to_owned(),
            description: "Available work".to_owned(),
            state: "open".to_owned(),
            agent_model_override: None,
            agent_reasoning_effort_override: None,
            initial_labels: Vec::new(),
        },
    )
    .await
    .unwrap();

    let claimed = claim_item(&store, "demo", "agent-a", "open")
        .await
        .unwrap()
        .unwrap();
    let comments = list_comments(&store, "demo", item.id).await.unwrap();
    let events = list_events(&store, "demo", Some(item.id), None)
        .await
        .unwrap();

    assert_eq!(claimed.id, item.id);
    assert_eq!(claimed.state.as_deref(), Some("in_progress"));
    assert_eq!(claimed.version, item.version + 1);
    assert_eq!(claimed.claimed_by.as_deref(), Some("agent-a"));
    assert!(claimed.claimed_at.is_some());
    assert!(
        comments
            .iter()
            .any(|comment| comment.body == "Claimed by agent-a")
    );
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "item_claimed")
    );
}

#[tokio::test]
async fn progress_records_agent_comment_and_touches_item() {
    let (_temp, store) = test_store().await;
    let item = create_item(
        &store,
        "demo",
        CreateWorkItem {
            title: "Progress target".to_owned(),
            description: "Progress should update visible item metadata".to_owned(),
            state: "open".to_owned(),
            agent_model_override: None,
            agent_reasoning_effort_override: None,
            initial_labels: Vec::new(),
        },
    )
    .await
    .unwrap();
    let claimed = claim_item(&store, "demo", "agent-a", "open")
        .await
        .unwrap()
        .unwrap();

    let comment = progress_item(&store, "demo", item.id, "agent-a", "Working")
        .await
        .unwrap();
    let updated = get_item(&store, "demo", item.id).await.unwrap();
    let events = list_events(&store, "demo", Some(item.id), None)
        .await
        .unwrap();

    assert_eq!(comment.author_type, AuthorType::Agent);
    assert_eq!(comment.author_name.as_deref(), Some("agent-a"));
    assert_eq!(comment.body, "Working");
    assert_eq!(updated.version, claimed.version + 1);
    assert_eq!(updated.comment_count, 2);
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "progress_added" && event.body == "Working")
    );
}

#[tokio::test]
async fn claiming_can_use_nested_label_conditions() {
    let (_temp, store) = test_store().await;
    create_item(
        &store,
        "demo",
        CreateWorkItem {
            title: "Plain item".to_owned(),
            description: "Should not match the selector".to_owned(),
            state: "open".to_owned(),
            agent_model_override: None,
            agent_reasoning_effort_override: None,
            initial_labels: Vec::new(),
        },
    )
    .await
    .unwrap();
    let matching = create_item(
        &store,
        "demo",
        CreateWorkItem {
            title: "Urgent bug".to_owned(),
            description: "Should match the selector".to_owned(),
            state: "open".to_owned(),
            agent_model_override: None,
            agent_reasoning_effort_override: None,
            initial_labels: Vec::new(),
        },
    )
    .await
    .unwrap();
    add_label(
        &store,
        "demo",
        matching.id,
        "severity".to_owned(),
        Some("high".to_owned()),
        None,
    )
    .await
    .unwrap();
    add_label(&store, "demo", matching.id, "bug".to_owned(), None, None)
        .await
        .unwrap();

    let selector = Condition::All(vec![
        ConditionElement::Clause(ConditionClause {
            column_name: "state".to_owned(),
            operator: Operator::Equal,
            value: ConditionClauseValue::String("open".to_owned()),
        }),
        ConditionElement::Condition(Box::new(Condition::Any(vec![
            ConditionElement::Clause(ConditionClause {
                column_name: "severity".to_owned(),
                operator: Operator::Equal,
                value: ConditionClauseValue::String("high".to_owned()),
            }),
            ConditionElement::Clause(ConditionClause {
                column_name: "bug".to_owned(),
                operator: Operator::Equal,
                value: ConditionClauseValue::Bool(true),
            }),
        ]))),
    ]);

    assert!(
        has_claimable_item_matching_condition(&store, "demo", &selector)
            .await
            .unwrap()
    );

    let claimed = claim_item_matching_condition(&store, "demo", "agent-a", &selector)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(claimed.id, matching.id);
    assert_eq!(claimed.claimed_by.as_deref(), Some("agent-a"));
    assert_eq!(claimed.state.as_deref(), Some("in_progress"));
}

#[tokio::test]
async fn blocked_items_are_skipped_by_selector_claims() {
    let (_temp, store) = test_store().await;
    let item = create_item(
        &store,
        "demo",
        CreateWorkItem {
            title: "Blocked bug".to_owned(),
            description: "Should wait for human triage".to_owned(),
            state: "open".to_owned(),
            agent_model_override: None,
            agent_reasoning_effort_override: None,
            initial_labels: Vec::new(),
        },
    )
    .await
    .unwrap();
    add_label(
        &store,
        "demo",
        item.id,
        AUTOMATION_BLOCKED_LABEL_KEY.to_owned(),
        None,
        None,
    )
    .await
    .unwrap();
    add_label(
        &store,
        "demo",
        item.id,
        "severity".to_owned(),
        Some("high".to_owned()),
        None,
    )
    .await
    .unwrap();

    let selector = Condition::All(vec![
        ConditionElement::Clause(ConditionClause {
            column_name: "state".to_owned(),
            operator: Operator::Equal,
            value: ConditionClauseValue::String("open".to_owned()),
        }),
        ConditionElement::Clause(ConditionClause {
            column_name: "severity".to_owned(),
            operator: Operator::Equal,
            value: ConditionClauseValue::String("high".to_owned()),
        }),
    ]);

    assert!(
        !has_claimable_item_matching_condition(&store, "demo", &selector)
            .await
            .unwrap()
    );
    let claimed = claim_item_matching_condition(&store, "demo", "agent-a", &selector)
        .await
        .unwrap();

    assert!(claimed.is_none());
}

#[tokio::test]
async fn claiming_is_atomic_for_racing_agents() {
    let (_temp, store) = test_store().await;
    create_item(
        &store,
        "demo",
        CreateWorkItem {
            title: "Race item".to_owned(),
            description: "Only one agent can own this".to_owned(),
            state: "open".to_owned(),
            agent_model_override: None,
            agent_reasoning_effort_override: None,
            initial_labels: Vec::new(),
        },
    )
    .await
    .unwrap();

    let (first, second) = tokio::join!(
        claim_item(&store, "demo", "agent-a", "open"),
        claim_item(&store, "demo", "agent-b", "open")
    );
    let claims = [first.unwrap(), second.unwrap()];
    let in_progress = list_items(&store, "demo", Some("in_progress".to_owned()))
        .await
        .unwrap();

    assert_eq!(claims.iter().filter(|claim| claim.is_some()).count(), 1);
    assert_eq!(in_progress.len(), 1);
    assert!(matches!(
        in_progress[0].claimed_by.as_deref(),
        Some("agent-a" | "agent-b")
    ));
}

#[tokio::test]
async fn claim_respects_project_scope() {
    let (_temp, store) = test_store().await;
    create_item(
        &store,
        "other",
        CreateWorkItem {
            title: "Other item".to_owned(),
            description: "Should not be claimed from demo".to_owned(),
            state: "open".to_owned(),
            agent_model_override: None,
            agent_reasoning_effort_override: None,
            initial_labels: Vec::new(),
        },
    )
    .await
    .unwrap();

    let claimed = claim_item(&store, "demo", "agent-a", "open").await.unwrap();

    assert!(claimed.is_none());
}

#[tokio::test]
async fn idea_item_is_skipped_until_moved_open() {
    let (_temp, store) = test_store().await;
    let item = create_item(
        &store,
        "demo",
        CreateWorkItem {
            title: "Draft item".to_owned(),
            description: "Hold this back from automation".to_owned(),
            state: "idea".to_owned(),
            agent_model_override: None,
            agent_reasoning_effort_override: None,
            initial_labels: Vec::new(),
        },
    )
    .await
    .unwrap();

    let skipped = claim_item(&store, "demo", "agent-a", "open").await.unwrap();
    assert!(skipped.is_none());

    let opened = move_item(
        &store,
        "demo",
        item.id,
        "open".to_owned(),
        Some(item.version),
    )
    .await
    .unwrap();
    let claimed = claim_item(&store, "demo", "agent-a", "open")
        .await
        .unwrap()
        .unwrap();

    assert_eq!(opened.state.as_deref(), Some("open"));
    assert_eq!(claimed.id, item.id);
    assert_eq!(claimed.claimed_by.as_deref(), Some("agent-a"));
}

#[tokio::test]
async fn claimed_items_include_verified_automation_source() {
    let (_temp, store) = test_store().await;
    let item = create_item(
        &store,
        "demo",
        CreateWorkItem {
            title: "Refine me".to_owned(),
            description: "A trigger should be visible while claimed".to_owned(),
            state: "open".to_owned(),
            agent_model_override: None,
            agent_reasoning_effort_override: None,
            initial_labels: Vec::new(),
        },
    )
    .await
    .unwrap();
    let now = utc_now();
    let run = agent_run::ActiveModel {
        project_id: Set(item.project_id),
        work_item_id: Set(Some(item.id)),
        memory_event_id: Set(None),
        trigger_id: Set(Some(7)),
        trigger_name: Set(Some("Refine queued item".to_owned())),
        tool_name: Set("codex".to_owned()),
        mutability: Set("read_only".to_owned()),
        status: Set("running".to_owned()),
        command: Set(String::new()),
        working_dir: Set(String::new()),
        worktree_path: Set(None),
        branch_name: Set(None),
        process_id: Set(None),
        exit_code: Set(None),
        log_path: Set(None),
        prompt_path: Set(None),
        agent_model: Set(None),
        agent_reasoning_effort: Set(None),
        input_tokens: Set(None),
        cached_input_tokens: Set(None),
        output_tokens: Set(None),
        commit_required: Set(false),
        commit_outcome: Set("not_evaluated".to_owned()),
        commit_shas: Set("[]".to_owned()),
        pr_requested: Set(false),
        pr_url: Set(None),
        cleanup_status: Set("not_applicable".to_owned()),
        worktree_cleaned_at: Set(None),
        result_summary: Set(String::new()),
        started_at: Set(Some(now.clone())),
        finished_at: Set(None),
        created_at: Set(now.clone()),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(store.db().as_ref())
    .await
    .unwrap();
    let agent_id = agent_ids::patchbay_run_agent_id(run.id);

    claim_specific_item(&store, "demo", item.id, &agent_id)
        .await
        .unwrap()
        .unwrap();
    let item = get_item(&store, "demo", item.id).await.unwrap();
    let listed = list_items(&store, "demo", None)
        .await
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == item.id)
        .unwrap();

    for view in [item, listed] {
        let claim_source = view.claim_source.expect("claim source should be present");
        assert_eq!(claim_source.run_id, run.id);
        assert_eq!(claim_source.trigger_id, Some(7));
        assert_eq!(
            claim_source.trigger_name.as_deref(),
            Some("Refine queued item")
        );
    }
}

#[tokio::test]
async fn claimed_items_ignore_unlinked_patchbay_run_claimants() {
    let (_temp, store) = test_store().await;
    let item = create_item(
        &store,
        "demo",
        CreateWorkItem {
            title: "Claim me".to_owned(),
            description: "Source should not be guessed from a mismatched run".to_owned(),
            state: "open".to_owned(),
            agent_model_override: None,
            agent_reasoning_effort_override: None,
            initial_labels: Vec::new(),
        },
    )
    .await
    .unwrap();
    let other = create_item(
        &store,
        "demo",
        CreateWorkItem {
            title: "Other".to_owned(),
            description: "The run is structurally linked here instead".to_owned(),
            state: "open".to_owned(),
            agent_model_override: None,
            agent_reasoning_effort_override: None,
            initial_labels: Vec::new(),
        },
    )
    .await
    .unwrap();
    let now = utc_now();
    let run = agent_run::ActiveModel {
        project_id: Set(item.project_id),
        work_item_id: Set(Some(other.id)),
        memory_event_id: Set(None),
        trigger_id: Set(Some(8)),
        trigger_name: Set(Some("Wrong source".to_owned())),
        tool_name: Set("codex".to_owned()),
        mutability: Set("mutating".to_owned()),
        status: Set("running".to_owned()),
        command: Set(String::new()),
        working_dir: Set(String::new()),
        worktree_path: Set(None),
        branch_name: Set(None),
        process_id: Set(None),
        exit_code: Set(None),
        log_path: Set(None),
        prompt_path: Set(None),
        agent_model: Set(None),
        agent_reasoning_effort: Set(None),
        input_tokens: Set(None),
        cached_input_tokens: Set(None),
        output_tokens: Set(None),
        commit_required: Set(false),
        commit_outcome: Set("not_evaluated".to_owned()),
        commit_shas: Set("[]".to_owned()),
        pr_requested: Set(false),
        pr_url: Set(None),
        cleanup_status: Set("not_applicable".to_owned()),
        worktree_cleaned_at: Set(None),
        result_summary: Set(String::new()),
        started_at: Set(Some(now.clone())),
        finished_at: Set(None),
        created_at: Set(now.clone()),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(store.db().as_ref())
    .await
    .unwrap();
    let agent_id = agent_ids::patchbay_run_agent_id(run.id);

    claim_specific_item(&store, "demo", item.id, &agent_id)
        .await
        .unwrap()
        .unwrap();
    let item = get_item(&store, "demo", item.id).await.unwrap();

    assert_eq!(item.claimed_by.as_deref(), Some(agent_id.as_str()));
    assert!(item.claim_source.is_none());
}

#[tokio::test]
async fn release_restores_claim_source_state_and_blocks_automation() {
    let (_temp, store) = test_store().await;
    let item = create_item(
        &store,
        "demo",
        CreateWorkItem {
            title: "Custom lane item".to_owned(),
            description: "Release should return to this lane".to_owned(),
            state: "ready".to_owned(),
            agent_model_override: None,
            agent_reasoning_effort_override: None,
            initial_labels: Vec::new(),
        },
    )
    .await
    .unwrap();

    let claimed = claim_item(&store, "demo", "agent-a", "ready")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claimed.state.as_deref(), Some(CLAIMED_STATE_LABEL));
    assert!(claimed.labels.iter().any(|label| {
        label.key == CLAIMED_FROM_STATE_LABEL_KEY && label.value.as_deref() == Some("ready")
    }));

    let released = release_item(
        &store,
        "demo",
        item.id,
        "agent-a",
        Some("Cannot operate on this item.".to_owned()),
        ReleaseAutomationDisposition::Blocked,
    )
    .await
    .unwrap();

    assert_eq!(released.state.as_deref(), Some("ready"));
    assert_eq!(released.claimed_by, None);
    assert!(
        released
            .labels
            .iter()
            .any(|label| label.key == AUTOMATION_BLOCKED_LABEL_KEY)
    );
    assert!(
        released
            .labels
            .iter()
            .all(|label| label.key != CLAIMED_FROM_STATE_LABEL_KEY)
    );

    let claimed_again = claim_item(&store, "demo", "agent-b", "ready")
        .await
        .unwrap();
    assert!(claimed_again.is_none());
}

#[tokio::test]
async fn claimable_release_clears_existing_automation_blocker() {
    let (_temp, store) = test_store().await;
    let item = create_item(
        &store,
        "demo",
        CreateWorkItem {
            title: "Manual retry".to_owned(),
            description: "A direct retry should be able to reopen automation.".to_owned(),
            state: "open".to_owned(),
            agent_model_override: None,
            agent_reasoning_effort_override: None,
            initial_labels: Vec::new(),
        },
    )
    .await
    .unwrap();
    add_label(
        &store,
        "demo",
        item.id,
        AUTOMATION_BLOCKED_LABEL_KEY.to_owned(),
        None,
        None,
    )
    .await
    .unwrap();

    let claimed = claim_specific_item(&store, "demo", item.id, "agent-a")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claimed.state.as_deref(), Some(CLAIMED_STATE_LABEL));
    assert!(
        claimed
            .labels
            .iter()
            .any(|label| label.key == AUTOMATION_BLOCKED_LABEL_KEY)
    );

    let released = release_item(
        &store,
        "demo",
        item.id,
        "agent-a",
        None,
        ReleaseAutomationDisposition::Claimable,
    )
    .await
    .unwrap();

    assert_eq!(released.state.as_deref(), Some("open"));
    assert_eq!(released.claimed_by, None);
    for key in [
        CLAIMED_FROM_STATE_LABEL_KEY,
        AUTOMATION_BLOCKED_LABEL_KEY,
        FEEDBACK_REQUESTED_LABEL_KEY,
    ] {
        assert!(
            released.labels.iter().all(|label| label.key != key),
            "claimable release should remove {key}"
        );
    }

    let claimed_again = claim_item(&store, "demo", "agent-b", "open")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claimed_again.id, item.id);
    assert_eq!(claimed_again.claimed_by.as_deref(), Some("agent-b"));
}

#[tokio::test]
async fn request_feedback_restores_source_state_and_blocks_automation() {
    let (_temp, store) = test_store().await;
    let item = create_item(
        &store,
        "demo",
        CreateWorkItem {
            title: "Needs input".to_owned(),
            description: "Agent should ask for a user decision".to_owned(),
            state: "ready".to_owned(),
            agent_model_override: None,
            agent_reasoning_effort_override: None,
            initial_labels: Vec::new(),
        },
    )
    .await
    .unwrap();
    claim_item(&store, "demo", "agent-a", "ready")
        .await
        .unwrap()
        .unwrap();

    let updated = request_feedback(
        &store,
        "demo",
        item.id,
        "agent-a",
        "Which provider should this integration target?",
    )
    .await
    .unwrap();
    let comments = list_comments(&store, "demo", item.id).await.unwrap();
    let events = list_events(&store, "demo", Some(item.id), None)
        .await
        .unwrap();

    assert_eq!(updated.state.as_deref(), Some("ready"));
    assert_eq!(updated.claimed_by, None);
    assert!(
        updated
            .labels
            .iter()
            .any(|label| label.key == AUTOMATION_BLOCKED_LABEL_KEY)
    );
    assert!(
        updated
            .labels
            .iter()
            .any(|label| label.key == FEEDBACK_REQUESTED_LABEL_KEY)
    );
    assert!(
        updated
            .labels
            .iter()
            .all(|label| label.key != CLAIMED_FROM_STATE_LABEL_KEY)
    );
    assert!(comments.iter().any(|comment| {
        comment.author_type == AuthorType::Agent
            && comment.author_name.as_deref() == Some("agent-a")
            && comment.body == "Which provider should this integration target?"
    }));
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "feedback_requested")
    );

    let claimed_again = claim_item(&store, "demo", "agent-b", "ready")
        .await
        .unwrap();
    assert!(claimed_again.is_none());
}

#[tokio::test]
async fn feedback_requested_label_blocks_state_claims() {
    let (_temp, store) = test_store().await;
    let item = create_item(
        &store,
        "demo",
        CreateWorkItem {
            title: "Awaiting answer".to_owned(),
            description: "Feedback label alone should block automation pickup".to_owned(),
            state: "open".to_owned(),
            agent_model_override: None,
            agent_reasoning_effort_override: None,
            initial_labels: Vec::new(),
        },
    )
    .await
    .unwrap();
    add_label(
        &store,
        "demo",
        item.id,
        FEEDBACK_REQUESTED_LABEL_KEY.to_owned(),
        None,
        None,
    )
    .await
    .unwrap();

    let claimed = claim_item(&store, "demo", "agent-a", "open").await.unwrap();

    assert!(claimed.is_none());
}

#[tokio::test]
async fn specific_claim_release_restores_current_state() {
    let (_temp, store) = test_store().await;
    let item = create_item(
        &store,
        "demo",
        CreateWorkItem {
            title: "Manual retry".to_owned(),
            description: "Explicit item claims are not tied to open".to_owned(),
            state: "triage".to_owned(),
            agent_model_override: None,
            agent_reasoning_effort_override: None,
            initial_labels: Vec::new(),
        },
    )
    .await
    .unwrap();

    let claimed = claim_specific_item(&store, "demo", item.id, "agent-a")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claimed.state.as_deref(), Some(CLAIMED_STATE_LABEL));

    let released = release_item(
        &store,
        "demo",
        item.id,
        "agent-a",
        None,
        ReleaseAutomationDisposition::Claimable,
    )
    .await
    .unwrap();

    assert_eq!(released.state.as_deref(), Some("triage"));
    assert_eq!(released.claimed_by, None);
    assert!(
        released
            .labels
            .iter()
            .all(|label| label.key != AUTOMATION_BLOCKED_LABEL_KEY)
    );

    let claimed_again = claim_item(&store, "demo", "agent-b", "triage")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claimed_again.id, item.id);
    assert_eq!(claimed_again.claimed_by.as_deref(), Some("agent-b"));
}

#[tokio::test]
async fn new_claims_overwrite_stale_claim_source_with_current_state() {
    let (_temp, store) = test_store().await;
    let state_item = create_item(
        &store,
        "demo",
        CreateWorkItem {
            title: "State retry".to_owned(),
            description: "State claims use the current state as release source".to_owned(),
            state: "open".to_owned(),
            agent_model_override: None,
            agent_reasoning_effort_override: None,
            initial_labels: Vec::new(),
        },
    )
    .await
    .unwrap();
    add_label(
        &store,
        "demo",
        state_item.id,
        CLAIMED_FROM_STATE_LABEL_KEY.to_owned(),
        Some("ready".to_owned()),
        None,
    )
    .await
    .unwrap();

    let claimed = claim_item(&store, "demo", "agent-state", "open")
        .await
        .unwrap()
        .unwrap();

    assert!(claimed.labels.iter().any(|label| {
        label.key == CLAIMED_FROM_STATE_LABEL_KEY && label.value.as_deref() == Some("open")
    }));

    let released = release_item(
        &store,
        "demo",
        state_item.id,
        "agent-state",
        None,
        ReleaseAutomationDisposition::Claimable,
    )
    .await
    .unwrap();

    assert_eq!(released.state.as_deref(), Some("open"));

    let selector_item = create_item(
        &store,
        "demo",
        CreateWorkItem {
            title: "Selector retry".to_owned(),
            description: "Claim source should come from the current state label".to_owned(),
            state: "ready".to_owned(),
            agent_model_override: None,
            agent_reasoning_effort_override: None,
            initial_labels: Vec::new(),
        },
    )
    .await
    .unwrap();
    add_label(
        &store,
        "demo",
        selector_item.id,
        CLAIMED_FROM_STATE_LABEL_KEY.to_owned(),
        Some("open".to_owned()),
        None,
    )
    .await
    .unwrap();

    let selector = Condition::All(vec![ConditionElement::Clause(ConditionClause {
        column_name: STATE_LABEL_KEY.to_owned(),
        operator: Operator::Equal,
        value: ConditionClauseValue::String("ready".to_owned()),
    })]);
    let claimed = claim_item_matching_condition(&store, "demo", "agent-a", &selector)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(claimed.state.as_deref(), Some(CLAIMED_STATE_LABEL));
    assert!(claimed.labels.iter().any(|label| {
        label.key == CLAIMED_FROM_STATE_LABEL_KEY && label.value.as_deref() == Some("ready")
    }));

    let released = release_item(
        &store,
        "demo",
        selector_item.id,
        "agent-a",
        None,
        ReleaseAutomationDisposition::Claimable,
    )
    .await
    .unwrap();

    assert_eq!(released.state.as_deref(), Some("ready"));

    let specific_item = create_item(
        &store,
        "demo",
        CreateWorkItem {
            title: "Specific retry".to_owned(),
            description: "Specific claims use the same source-state rule".to_owned(),
            state: "triage".to_owned(),
            agent_model_override: None,
            agent_reasoning_effort_override: None,
            initial_labels: Vec::new(),
        },
    )
    .await
    .unwrap();
    add_label(
        &store,
        "demo",
        specific_item.id,
        CLAIMED_FROM_STATE_LABEL_KEY.to_owned(),
        Some("open".to_owned()),
        None,
    )
    .await
    .unwrap();

    let claimed = claim_specific_item(&store, "demo", specific_item.id, "agent-b")
        .await
        .unwrap()
        .unwrap();

    assert!(claimed.labels.iter().any(|label| {
        label.key == CLAIMED_FROM_STATE_LABEL_KEY && label.value.as_deref() == Some("triage")
    }));

    let released = release_item(
        &store,
        "demo",
        specific_item.id,
        "agent-b",
        None,
        ReleaseAutomationDisposition::Claimable,
    )
    .await
    .unwrap();

    assert_eq!(released.state.as_deref(), Some("triage"));
}

#[tokio::test]
async fn release_requires_current_claimant() {
    let (_temp, store) = test_store().await;
    let item = create_item(
        &store,
        "demo",
        CreateWorkItem {
            title: "Owned item".to_owned(),
            description: "Only the claimant can release it".to_owned(),
            state: "open".to_owned(),
            agent_model_override: None,
            agent_reasoning_effort_override: None,
            initial_labels: Vec::new(),
        },
    )
    .await
    .unwrap();
    claim_item(&store, "demo", "agent-a", "open")
        .await
        .unwrap()
        .unwrap();

    let err = release_item(
        &store,
        "demo",
        item.id,
        "agent-b",
        None,
        ReleaseAutomationDisposition::Blocked,
    )
    .await
    .unwrap_err();

    assert!(err.to_string().contains("claimed by agent-a"));
}

#[tokio::test]
async fn finish_clears_claim_and_blocking_workflow_labels() {
    let (_temp, store) = test_store().await;
    let item = create_item(
        &store,
        "demo",
        CreateWorkItem {
            title: "Finish blocked item".to_owned(),
            description: "Completion should clear workflow bookkeeping labels".to_owned(),
            state: "ready".to_owned(),
            agent_model_override: None,
            agent_reasoning_effort_override: None,
            initial_labels: Vec::new(),
        },
    )
    .await
    .unwrap();
    add_label(
        &store,
        "demo",
        item.id,
        AUTOMATION_BLOCKED_LABEL_KEY.to_owned(),
        None,
        None,
    )
    .await
    .unwrap();
    claim_specific_item(&store, "demo", item.id, "agent-a")
        .await
        .unwrap()
        .unwrap();
    add_label(
        &store,
        "demo",
        item.id,
        FEEDBACK_REQUESTED_LABEL_KEY.to_owned(),
        None,
        None,
    )
    .await
    .unwrap();

    let finished = finish_item(&store, "demo", item.id, "agent-a", "Finished cleanly")
        .await
        .unwrap();

    assert_eq!(finished.state.as_deref(), Some(FINISHED_STATE_LABEL));
    assert_eq!(finished.claimed_by, None);
    assert!(finished.finished_at.is_some());
    for key in [
        CLAIMED_FROM_STATE_LABEL_KEY,
        AUTOMATION_BLOCKED_LABEL_KEY,
        FEEDBACK_REQUESTED_LABEL_KEY,
    ] {
        assert!(
            finished.labels.iter().all(|label| label.key != key),
            "finished item should not retain {key}"
        );
    }
}

#[tokio::test]
async fn finish_moves_done_and_records_report() {
    let (_temp, store) = test_store().await;
    let item = create_item(
        &store,
        "demo",
        CreateWorkItem {
            title: "Finish item".to_owned(),
            description: "Complete with report".to_owned(),
            state: "open".to_owned(),
            agent_model_override: None,
            agent_reasoning_effort_override: None,
            initial_labels: Vec::new(),
        },
    )
    .await
    .unwrap();
    claim_item(&store, "demo", "agent-a", "open")
        .await
        .unwrap()
        .unwrap();

    let finished = finish_item(&store, "demo", item.id, "agent-a", "Finished cleanly")
        .await
        .unwrap();
    let comments = list_comments(&store, "demo", item.id).await.unwrap();
    let events = list_events(&store, "demo", Some(item.id), None)
        .await
        .unwrap();

    assert_eq!(finished.state.as_deref(), Some("done"));
    assert_eq!(finished.claimed_by, None);
    assert!(finished.finished_at.is_some());
    assert!(
        comments
            .iter()
            .any(|comment| comment.body == "Finished cleanly")
    );
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "item_finished")
    );
}

#[tokio::test]
async fn state_and_selector_claims_do_not_reopen_finished_items() {
    let (_temp, store) = test_store().await;
    let item = create_item(
        &store,
        "demo",
        CreateWorkItem {
            title: "Finished item".to_owned(),
            description: "State changes alone should not reopen finished work".to_owned(),
            state: "open".to_owned(),
            agent_model_override: None,
            agent_reasoning_effort_override: None,
            initial_labels: Vec::new(),
        },
    )
    .await
    .unwrap();
    claim_item(&store, "demo", "agent-a", "open")
        .await
        .unwrap()
        .unwrap();
    let finished = finish_item(&store, "demo", item.id, "agent-a", "Finished")
        .await
        .unwrap();
    let moved = move_item(
        &store,
        "demo",
        item.id,
        "open".to_owned(),
        Some(finished.version),
    )
    .await
    .unwrap();
    let selector = Condition::All(vec![ConditionElement::Clause(ConditionClause {
        column_name: STATE_LABEL_KEY.to_owned(),
        operator: Operator::Equal,
        value: ConditionClauseValue::String("open".to_owned()),
    })]);

    let state_claim = claim_item(&store, "demo", "agent-b", "open").await.unwrap();
    let selector_has_match = has_claimable_item_matching_condition(&store, "demo", &selector)
        .await
        .unwrap();
    let selector_claim = claim_item_matching_condition(&store, "demo", "agent-c", &selector)
        .await
        .unwrap();
    let reloaded = get_item(&store, "demo", item.id).await.unwrap();

    assert_eq!(moved.state.as_deref(), Some("open"));
    assert!(moved.finished_at.is_some());
    assert!(state_claim.is_none());
    assert!(!selector_has_match);
    assert!(selector_claim.is_none());
    assert_eq!(reloaded.claimed_by, None);
    assert!(reloaded.finished_at.is_some());
}

#[tokio::test]
async fn specific_claim_does_not_reopen_finished_items() {
    let (_temp, store) = test_store().await;
    let item = create_item(
        &store,
        "demo",
        CreateWorkItem {
            title: "Finished item".to_owned(),
            description: "Should stay closed after completion".to_owned(),
            state: "open".to_owned(),
            agent_model_override: None,
            agent_reasoning_effort_override: None,
            initial_labels: Vec::new(),
        },
    )
    .await
    .unwrap();
    claim_item(&store, "demo", "agent-a", "open")
        .await
        .unwrap()
        .unwrap();
    finish_item(&store, "demo", item.id, "agent-a", "Finished")
        .await
        .unwrap();

    let claimed = claim_specific_item(&store, "demo", item.id, "agent-b")
        .await
        .unwrap();
    let reloaded = get_item(&store, "demo", item.id).await.unwrap();

    assert!(claimed.is_none());
    assert_eq!(reloaded.state.as_deref(), Some(FINISHED_STATE_LABEL));
    assert_eq!(reloaded.claimed_by, None);
    assert!(reloaded.finished_at.is_some());
}

#[tokio::test]
async fn stale_claim_recovery_releases_old_claim() {
    let (_temp, store) = test_store().await;
    let item = create_item(
        &store,
        "demo",
        CreateWorkItem {
            title: "Stale item".to_owned(),
            description: "Claim should be recovered".to_owned(),
            state: "open".to_owned(),
            agent_model_override: None,
            agent_reasoning_effort_override: None,
            initial_labels: Vec::new(),
        },
    )
    .await
    .unwrap();
    claim_item(&store, "demo", "agent-a", "open")
        .await
        .unwrap()
        .unwrap();
    let project_id = projects::project_id(&store, "demo").await.unwrap();
    let mut model: WorkItemActiveModel = work_items::get(store.db().as_ref(), project_id, item.id)
        .await
        .unwrap()
        .into();
    model.claimed_at = Set(Some(
        (OffsetDateTime::now_utc() - Duration::minutes(30))
            .format(&Rfc3339)
            .unwrap(),
    ));
    model.update(store.db().as_ref()).await.unwrap();

    let recovered = recover_stale_claims(&store, "demo", 10).await.unwrap();
    let item = get_item(&store, "demo", item.id).await.unwrap();

    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].agent_id, "agent-a");
    assert_eq!(item.state.as_deref(), Some("open"));
    assert_eq!(item.claimed_by, None);
}
