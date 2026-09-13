use super::restore_thread_environments;
use crate::session::tests::make_session_and_context;
use codex_exec_server::EnvironmentManager;
use codex_history::InitialHistory;
use codex_history::ResumedHistory;
use codex_history::RolloutItem;
use codex_protocol::ThreadId;
use codex_protocol::protocol::EnvironmentConfigState;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::ThreadSettingsAppliedEvent;
use codex_protocol::protocol::TurnEnvironmentSelection;
use codex_utils_path_uri::PathUri;
use pretty_assertions::assert_eq;
use std::sync::Arc;

#[tokio::test]
async fn resume_uses_only_the_latest_thread_owned_environment_settings() {
    let (session, context) = make_session_and_context().await;
    let mut snapshot = session.thread_settings_snapshot().await;
    snapshot.environments = Some(vec![]);
    let thread_id = session.thread_id();
    let item = RolloutItem::EventMsg(EventMsg::ThreadSettingsApplied(
        ThreadSettingsAppliedEvent {
            thread_id: Some(thread_id),
            thread_settings: snapshot.clone(),
        },
    ));
    let mut resumed = ResumedHistory {
        conversation_id: thread_id,
        history: Arc::new(vec![item]),
        rollout_path: None,
    };
    let manager = EnvironmentManager::default_for_tests();
    assert_eq!(
        restore_thread_environments(
            &InitialHistory::Resumed(resumed.clone()),
            &context.config,
            &manager
        )
        .unwrap(),
        Some(vec![])
    );

    // A copied parent's later record must not replace this thread's selection.
    snapshot.environments = None;
    Arc::make_mut(&mut resumed.history).push(RolloutItem::EventMsg(
        EventMsg::ThreadSettingsApplied(ThreadSettingsAppliedEvent {
            thread_id: Some(ThreadId::new()),
            thread_settings: snapshot.clone(),
        }),
    ));
    assert_eq!(
        restore_thread_environments(
            &InitialHistory::Resumed(resumed.clone()),
            &context.config,
            &manager
        )
        .unwrap(),
        Some(vec![])
    );

    // A newer snapshot written by an older binary has no selections. Use legacy
    // defaults, rather than reviving older selections that may no longer apply.
    Arc::make_mut(&mut resumed.history).push(RolloutItem::EventMsg(
        EventMsg::ThreadSettingsApplied(ThreadSettingsAppliedEvent {
            thread_id: Some(thread_id),
            thread_settings: snapshot,
        }),
    ));
    assert_eq!(
        restore_thread_environments(&InitialHistory::Resumed(resumed), &context.config, &manager)
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn resume_retargets_cwd_roots_without_discarding_additional_roots() {
    let (session, context) = make_session_and_context().await;
    let mut config = context.config.as_ref().clone();
    let mut snapshot = session.thread_settings_snapshot().await;
    let moved_cwd = snapshot.cwd.join("moved");
    let shared_root = PathUri::from_abs_path(&snapshot.cwd.join("shared"));
    let selection = TurnEnvironmentSelection {
        environment_id: "local".into(),
        cwd: PathUri::from_abs_path(&snapshot.cwd),
        workspace_roots: vec![
            PathUri::from_abs_path(&snapshot.cwd),
            PathUri::from_abs_path(&moved_cwd),
            shared_root.clone(),
        ],
        config: EnvironmentConfigState::FromThread,
    };
    snapshot.environments = Some(vec![(&selection).into()]);
    let history = InitialHistory::Resumed(ResumedHistory {
        conversation_id: session.thread_id(),
        history: Arc::new(vec![RolloutItem::EventMsg(
            EventMsg::ThreadSettingsApplied(ThreadSettingsAppliedEvent {
                thread_id: Some(session.thread_id()),
                thread_settings: snapshot,
            }),
        )]),
        rollout_path: None,
    });
    config.cwd = moved_cwd.clone();
    config.workspace_roots_explicit = false;
    let manager = EnvironmentManager::default_for_tests();
    assert_eq!(
        restore_thread_environments(&history, &config, &manager).unwrap(),
        Some(vec![TurnEnvironmentSelection {
            cwd: PathUri::from_abs_path(&moved_cwd),
            workspace_roots: vec![PathUri::from_abs_path(&moved_cwd), shared_root],
            ..selection
        }])
    );
}
