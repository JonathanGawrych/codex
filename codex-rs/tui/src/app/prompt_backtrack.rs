//! Apply earlier-prompt edits after the transcript selector confirms a target.

use super::*;
use crate::app_backtrack::BacktrackTurnTarget;
use crate::app_backtrack::backtrack_turn_target;
use crate::app_event::PromptBacktrackAction;
use crate::app_server_session::ForkGoalContinuation;
use crate::chatwidget::UserMessage;

impl App {
    pub(super) fn config_for_fork(&self) -> Config {
        let mut config = self.config.clone();
        if self.app_server_target.uses_remote_workspace() {
            config
                .workspace_roots
                .clone_from(&self.chat_widget.config_ref().workspace_roots);
        }
        config.model = Some(self.chat_widget.current_model().to_string());
        config.model_reasoning_effort = self.chat_widget.current_reasoning_effort();
        config.service_tier = self.chat_widget.configured_service_tier();
        config
    }

    pub(super) async fn edit_earlier_prompt(
        &mut self,
        tui: &mut tui::Tui,
        app_server: &mut AppServerSession,
        thread_id: ThreadId,
        selected_cell: Arc<dyn HistoryCell>,
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

        let Some(index) = self
            .transcript_cells
            .iter()
            .position(|cell| Arc::ptr_eq(cell, &selected_cell))
        else {
            self.restore_backtrack_prompt_after_branch_error(
                prompt,
                "the selected prompt is no longer visible",
            );
            tui.frame_requester().schedule_frame();
            return;
        };
        let nth_user_message = crate::app_backtrack::user_count(&self.transcript_cells[..index]);
        self.refresh_in_memory_config_from_disk_best_effort("editing an earlier prompt")
            .await;
        let config = self.config_for_fork();
        let target = self
            .read_prompt_edit_history(app_server, thread_id)
            .await
            .and_then(|(turns, start_item)| {
                backtrack_turn_target(&turns, start_item.as_ref(), nth_user_message, &mut prompt)
            });

        match action {
            PromptBacktrackAction::Fork => match target {
                Ok(target) => {
                    self.fork_for_prompt_edit(app_server, config, thread_id, target, prompt)
                        .await;
                }
                Err(err) => {
                    self.restore_backtrack_prompt_after_branch_error(prompt, err);
                }
            },
        }
        tui.frame_requester().schedule_frame();
    }

    pub(super) async fn read_prompt_edit_history(
        &self,
        app_server: &mut AppServerSession,
        thread_id: ThreadId,
    ) -> Result<(Vec<Turn>, Option<(String, String)>)> {
        let channel = self
            .thread_event_channels
            .get(&thread_id)
            .ok_or_else(|| color_eyre::eyre::eyre!("the selected thread is no longer available"))?;
        let (start_item, loaded_tail, latest_turn_id) = {
            let store = channel.store.lock().await;
            (
                store.turns.iter().find_map(|turn| {
                    turn.items
                        .first()
                        .map(|item| (turn.id.clone(), item.id().to_string()))
                }),
                store.turns.last().map(|turn| turn.id.clone()),
                store.latest_turn_id.clone(),
            )
        };
        let mut thread = app_server
            .thread_read(thread_id, /*include_turns*/ false)
            .await?;
        app_server
            .hydrate_initial_thread_history(
                &mut thread,
                /*turn_cursor*/ None,
                /*item_cursor*/ None,
                /*config*/ None,
                /*local_settings*/ None,
                start_item
                    .as_ref()
                    .map(|(turn_id, _)| turn_id)
                    .or(loaded_tail.as_ref())
                    .map_or(
                        crate::app_server_session::HistoryHydrationScope::Complete,
                        |turn_id| {
                            crate::app_server_session::HistoryHydrationScope::ThroughTurn(turn_id)
                        },
                    ),
            )
            .await?;
        if thread.turns.last().map(|turn| &turn.id) != latest_turn_id.as_ref() {
            color_eyre::eyre::bail!(
                "thread history changed; reload the session before editing this prompt"
            );
        }
        // With no retained visible items, the next prompt follows the metadata-only tail.
        let start_item = start_item.or_else(|| {
            loaded_tail.as_ref().and_then(|tail| {
                thread
                    .turns
                    .iter()
                    .position(|turn| &turn.id == tail)
                    .and_then(|index| {
                        thread.turns[index + 1..].iter().find_map(|turn| {
                            turn.items
                                .first()
                                .map(|item| (turn.id.clone(), item.id().to_string()))
                        })
                    })
            })
        });
        Ok((thread.turns, start_item))
    }

    async fn fork_for_prompt_edit(
        &mut self,
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
        let starts_empty =
            !target.has_loaded_turn_before && !app_server.has_older_history(thread_id);
        let started = if !starts_empty {
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
                    &config,
                    /*session_start_source*/ None,
                    /*remote_cwd_override*/ None,
                    selected_profile.as_ref(),
                )
                .await
        };

        match started {
            Ok(forked) => {
                let fork_name = super::fork_terminal::forked_thread_name(
                    self.chat_widget.thread_name().as_deref(),
                    /*explicit_name*/ None,
                    Some("fork for prompt edit"),
                );
                let fork_thread_id = forked.session.thread_id;
                let name_error = app_server
                    .thread_set_name(fork_thread_id, fork_name.clone())
                    .await
                    .err()
                    .map(|err| format!("Failed to name the forked session: {err}"));
                if let Some(err) = name_error {
                    self.chat_widget.add_error_message(err);
                }
                // Naming saves metadata; reading history then persists an empty paginated root
                // so the new terminal can resume it before any user prompt has been submitted.
                if starts_empty
                    && let Err(error) = app_server
                        .thread_read(fork_thread_id, /*include_turns*/ true)
                        .await
                {
                    self.restore_backtrack_prompt_after_branch_error(prompt, error);
                    return;
                }
                let profile = self
                    .loader_overrides
                    .user_config_profile
                    .as_ref()
                    .map(|profile| profile.as_str().to_string());
                match super::fork_terminal::open_forked_session(
                    fork_thread_id,
                    profile.as_deref(),
                    &self.app_server_target,
                    Some(prompt.clone()),
                )
                .await
                {
                    Ok(()) => self.chat_widget.add_plain_history_lines(vec![
                        vec![
                            "Forked ".into(),
                            fork_name.cyan(),
                            " in a new terminal with the selected prompt ready to edit.".into(),
                        ]
                        .into(),
                    ]),
                    Err(err) => {
                        self.restore_backtrack_prompt_after_branch_error(prompt, err);
                    }
                }
            }
            Err(err) => self.restore_backtrack_prompt_after_branch_error(prompt, err),
        }
    }
}
