use super::*;
use crate::status_line_command::StatusLineReloadCredit;
use crate::status_line_command::StatusLineReloadCredits;
use codex_app_server_protocol::ConsumeAccountRateLimitResetCreditOutcome;
use codex_app_server_protocol::ConsumeAccountRateLimitResetCreditResponse;
use codex_app_server_protocol::GetAccountRateLimitsResponse;
use pretty_assertions::assert_eq;
use serde_json::json;

#[tokio::test]
async fn status_line_reloads_follow_accepted_reads_and_survive_thread_switches() -> Result<()> {
    let (mut app, _events, _ops) = make_test_app_with_channels().await;
    let mut session = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    let response: GetAccountRateLimitsResponse = serde_json::from_value(json!({
        "rateLimits": {}, "rateLimitsByLimitId": null, "accountId": null,
        "rateLimitUpsell": null,
        "rateLimitResetCredits": {"availableCount": 2, "credits": [
            {"id":"test-reload", "resetType":"codexRateLimits", "status":"available", "grantedAt":1, "expiresAt":1_801_000_000}
        ]}
    }))?;
    let expected = Some(StatusLineReloadCredits {
        available_count: 2,
        credits: Some(vec![StatusLineReloadCredit {
            expires_at: Some(1_801_000_000),
        }]),
    });
    let generation = app.rate_limit_hard_stop_generation;
    app.handle_event(
        &mut tui,
        &mut session,
        AppEvent::RateLimitsLoaded {
            request_id: 2,
            origin: RateLimitRefreshOrigin::StartupPrefetch {
                reset_hint_request_id: 0,
            },
            hard_stop_generation: generation,
            result: Ok(response.clone()),
        },
    )
    .await?;
    assert_eq!(app.chat_widget.status_line_reload_credits, expected);

    let init = app.chatwidget_init_for_forked_or_resumed_thread(
        &mut tui,
        app.config.clone(),
        /*initial_user_message*/ None,
    );
    app.replace_chat_widget(ChatWidget::new_with_app_event(init));
    assert_eq!(app.chat_widget.status_line_reload_credits, expected);

    let mut without_credits = response.clone();
    without_credits.rate_limit_reset_credits = None;
    for (request_id, hard_stop_generation, result) in [
        (1, generation, Ok(without_credits.clone())),
        (3, generation, Err("account read failed".to_string())),
        (4, generation.wrapping_add(1), Ok(without_credits.clone())),
    ] {
        app.handle_event(
            &mut tui,
            &mut session,
            AppEvent::RateLimitsLoaded {
                request_id,
                origin: RateLimitRefreshOrigin::StatusCommand { request_id: 0 },
                hard_stop_generation,
                result,
            },
        )
        .await?;
        assert_eq!(app.chat_widget.status_line_reload_credits, expected);
    }

    app.handle_event(
        &mut tui,
        &mut session,
        AppEvent::RateLimitsLoaded {
            request_id: 5,
            origin: RateLimitRefreshOrigin::StatusCommand { request_id: 0 },
            hard_stop_generation: generation,
            result: Ok(without_credits),
        },
    )
    .await?;
    assert_eq!(app.chat_widget.status_line_reload_credits, None);

    app.handle_event(
        &mut tui,
        &mut session,
        AppEvent::RateLimitsLoaded {
            request_id: 6,
            origin: RateLimitRefreshOrigin::StatusCommand { request_id: 0 },
            hard_stop_generation: generation,
            result: Ok(response),
        },
    )
    .await?;
    assert_eq!(app.chat_widget.status_line_reload_credits, expected);
    app.chat_widget.update_account_state(
        /*status_account_display*/ None, /*plan_type*/ None,
        /*has_chatgpt_account*/ false, /*has_codex_backend_auth*/ false,
    );
    assert_eq!(app.chat_widget.status_line_reload_credits, None);
    session.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn status_line_reloads_clear_after_redemption_until_refreshed() {
    let (mut app, _events, _ops) = make_test_app_with_channels().await;
    for outcome in [
        ConsumeAccountRateLimitResetCreditOutcome::Reset,
        ConsumeAccountRateLimitResetCreditOutcome::AlreadyRedeemed,
        ConsumeAccountRateLimitResetCreditOutcome::NoCredit,
    ] {
        app.chat_widget.status_line_reload_credits = Some(StatusLineReloadCredits {
            available_count: 1,
            credits: None,
        });
        let request_id = app.chat_widget.show_rate_limit_reset_consuming_popup();
        app.chat_widget.finish_rate_limit_reset_consume(
            request_id,
            "test-reset".into(),
            /*credit_id*/ None,
            Ok(ConsumeAccountRateLimitResetCreditResponse { outcome }),
        );
        assert_eq!(app.chat_widget.status_line_reload_credits, None);
    }
}
