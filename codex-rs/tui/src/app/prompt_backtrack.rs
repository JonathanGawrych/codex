//! Apply earlier-prompt edits after the transcript selector confirms a target.

use super::session_lifecycle::ThreadAttachPresentation;
use super::*;
use crate::app_backtrack::BacktrackTurnTarget;
use crate::app_backtrack::backtrack_turn_target;
use crate::app_event::PromptBacktrackAction;
use crate::app_server_session::ForkGoalContinuation;
use crate::app_server_session::ResumeModelSettings;
use crate::chatwidget::UserMessage;
use codex_app_server_protocol::ThreadHistoryMode;

impl App {
    pub(super) async fn edit_earlier_prompt(
        &mut self,
        tui: &mut tui::Tui,
        app_server: &mut AppServerSession,
        thread_id: ThreadId,
        nth_user_message: usize,
        mut prompt: UserMessage,
        action: PromptBacktrackAction,
    ) {
        if self.chat_widget.thread_id() != Some(thread_id) {
            return;
        }
        if self.pending_server_profiles.contains_key(&thread_id) {
            self.chat_widget.restore_user_message_to_composer(prompt);
            self.chat_widget.add_error_message(
                "Wait for permissions to update before editing this prompt.".into(),
            );
            tui.frame_requester().schedule_frame();
            return;
        }

        self.refresh_in_memory_config_from_disk_best_effort("editing an earlier prompt")
            .await;
        let config = self.fresh_session_config();
        let target = self
            .turns_for_earlier_prompt_edit(thread_id)
            .await
            .and_then(|turns| backtrack_turn_target(&turns, nth_user_message, &mut prompt));

        match action {
            PromptBacktrackAction::Rollback => match target {
                Ok(target) => {
                    self.rollback_for_prompt_edit(
                        tui, app_server, config, thread_id, target, prompt,
                    )
                    .await;
                }
                Err(err) => {
                    self.restore_backtrack_prompt_after_rollback_error(prompt, err);
                }
            },
            PromptBacktrackAction::Fork => match target {
                Ok(target) => {
                    self.fork_for_prompt_edit(tui, app_server, config, thread_id, target, prompt)
                        .await;
                }
                Err(err) => {
                    self.restore_backtrack_prompt_after_branch_error(prompt, err);
                }
            },
        }
        tui.frame_requester().schedule_frame();
    }

    async fn turns_for_earlier_prompt_edit(&self, thread_id: ThreadId) -> Result<Vec<Turn>> {
        let channel = self.thread_event_channels.get(&thread_id).ok_or_else(|| {
            color_eyre::eyre::eyre!("the selected thread is no longer available for prompt editing")
        })?;
        let store = channel.store.lock().await;
        let mut turns = store.turns.clone();
        // Snapshot turns contain loaded history; newer live turns remain in the replay buffer and
        // must also be visible when resolving the selected transcript prompt.
        for event in &store.buffer {
            let ThreadBufferedEvent::Notification(notification) = event else {
                continue;
            };
            match notification.as_ref() {
                ServerNotification::TurnStarted(notification)
                    if !turns.iter().any(|turn| turn.id == notification.turn.id) =>
                {
                    turns.push(notification.turn.clone());
                }
                ServerNotification::ItemCompleted(notification) => {
                    if matches!(
                        notification.item,
                        ThreadItem::UserMessage { .. }
                            | ThreadItem::EnteredReviewMode { .. }
                            | ThreadItem::ExitedReviewMode { .. }
                    ) && let Some(turn) = turns
                        .iter_mut()
                        .find(|turn| turn.id == notification.turn_id)
                        && !turn
                            .items
                            .iter()
                            .any(|item| item.id() == notification.item.id())
                    {
                        turn.items.push(notification.item.clone());
                    }
                }
                ServerNotification::TurnCompleted(notification) => {
                    if let Some(turn) = turns
                        .iter_mut()
                        .find(|turn| turn.id == notification.turn.id)
                    {
                        turn.status = notification.turn.status.clone();
                        turn.error = notification.turn.error.clone();
                        turn.started_at = notification.turn.started_at;
                        turn.completed_at = notification.turn.completed_at;
                        turn.duration_ms = notification.turn.duration_ms;
                    }
                }
                _ => {}
            }
        }
        Ok(turns)
    }

    async fn fork_for_prompt_edit(
        &mut self,
        tui: &mut tui::Tui,
        app_server: &mut AppServerSession,
        config: Config,
        thread_id: ThreadId,
        target: BacktrackTurnTarget,
        prompt: UserMessage,
    ) {
        self.session_telemetry.counter(
            "codex.thread.fork",
            /*inc*/ 1,
            &[("source", "transcript")],
        );
        let selected_profile = self.confirmed_server_profile(thread_id);
        let started = if target.has_loaded_turn_before || app_server.has_older_history(thread_id) {
            app_server
                .fork_thread_at(
                    &self.local_settings,
                    config,
                    thread_id,
                    /*last_turn_id*/ None,
                    Some(target.before_turn_id),
                    ForkGoalContinuation::StartIfIdle,
                    selected_profile.as_ref(),
                )
                .await
        } else {
            app_server
                .start_thread_with_session_start_source(
                    &self.local_settings,
                    &config, /*session_start_source*/ None, /*remote_cwd_override*/ None,
                    selected_profile.as_ref(),
                )
                .await
        };

        match started {
            Ok(forked) => {
                self.shutdown_current_thread(app_server).await;
                match self
                    .replace_chat_widget_with_app_server_thread(
                        tui,
                        forked,
                        ThreadAttachPresentation::PromptEdit,
                        /*initial_user_message*/ None,
                    )
                    .await
                {
                    Ok(()) => self.chat_widget.restore_user_message_to_composer(prompt),
                    Err(err) => self.restore_backtrack_prompt_after_branch_error(prompt, err),
                }
            }
            Err(err) => self.restore_backtrack_prompt_after_branch_error(prompt, err),
        }
    }

    async fn rollback_for_prompt_edit(
        &mut self,
        tui: &mut tui::Tui,
        app_server: &mut AppServerSession,
        config: Config,
        thread_id: ThreadId,
        target: BacktrackTurnTarget,
        prompt: UserMessage,
    ) {
        self.session_telemetry.counter(
            "codex.thread.rollback",
            /*inc*/ 1,
            &[("source", "transcript")],
        );
        let rollback = match app_server
            .thread_read(thread_id, /*include_turns*/ false)
            .await
        {
            Ok(thread) => match thread.history_mode {
                ThreadHistoryMode::Paginated => app_server
                    .thread_revert(thread_id, target.before_turn_id)
                    .await
                    .map(|_| ()),
                ThreadHistoryMode::Legacy => app_server
                    .thread_rollback(thread_id, target.turns_to_remove)
                    .await
                    .map(|_| ()),
            },
            Err(err) => Err(err),
        };
        if let Err(err) = rollback {
            self.restore_backtrack_prompt_after_rollback_error(prompt, err);
            return;
        }

        let resumed = match app_server
            .resume_thread(
                &self.local_settings,
                config,
                thread_id,
                ResumeModelSettings::PreserveExistingThread,
            )
            .await
        {
            Ok(resumed) => resumed,
            Err(err) => {
                self.restore_backtrack_prompt_after_rollback_refresh_error(prompt, err);
                return;
            }
        };
        if let Err(err) = self.reset_for_thread_switch(tui) {
            self.restore_backtrack_prompt_after_rollback_refresh_error(prompt, err);
            return;
        }
        match self
            .replace_chat_widget_with_app_server_thread(
                tui,
                resumed,
                ThreadAttachPresentation::PromptRollback,
                /*initial_user_message*/ None,
            )
            .await
        {
            Ok(()) => self.chat_widget.restore_user_message_to_composer(prompt),
            Err(err) => self.restore_backtrack_prompt_after_rollback_refresh_error(prompt, err),
        }
    }
}
