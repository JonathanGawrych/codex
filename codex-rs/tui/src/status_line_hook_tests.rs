use super::run_status_line_command;
use codex_config::types::StatusLineCommandConfig;
use codex_protocol::config_types::ServiceTier;
use ratatui::text::Line;
use serde_json::Value;
use serde_json::json;
use std::path::Path;

const NOW: i64 = 1_800_000_000;

#[allow(clippy::expect_used)]
async fn render_hook(payload: Value) -> Line<'static> {
    let script = codex_utils_cargo_bin::find_resource!("../../scripts/codex-statusline.sh")
        .expect("status-line hook resource");
    let script = script.to_str().expect("status-line hook path is UTF-8");
    let source_command = format!("date() {{ printf '%s\\n' {NOW}; }}; source \"$1\"");
    let command = shlex::try_join(["bash", "-c", &source_command, "--", script])
        .expect("status-line hook command");
    run_status_line_command(
        StatusLineCommandConfig {
            command,
            timeout_ms: 3_000,
            refresh_interval_seconds: 60,
        },
        payload.to_string(),
        Path::new("/"),
    )
    .await
    .expect("hook should succeed")
    .expect("hook should render one line")
}

fn payload() -> Value {
    json!({
        "workspace": {"current_dir": "/Volumes/code/external"},
        "model": {"display_name": "gpt-5.6-sol", "reasoning_effort": "xhigh"},
        "service_tier": ServiceTier::Fast.request_value(),
        "personality": {"id": "pragmatic", "display_name": "Pragmatic"},
        "context_window": {"used_percentage": 11},
    })
}

#[tokio::test]
async fn fast_mode_uses_current_and_legacy_service_tiers() {
    let mut output = String::new();
    for tier in [
        Some(ServiceTier::Fast.request_value()),
        Some("fast"),
        Some("default"),
        Some("flex"),
        None,
    ] {
        let mut payload = payload();
        payload["service_tier"] = json!(tier);
        let line = render_hook(payload).await;
        output.push_str(&format!("{tier:?}: {line}\n"));
    }
    insta::assert_snapshot!(output);
}

#[tokio::test]
async fn pace_distinguishes_overspending_from_spare_allowance() {
    let mut output = String::new();
    for (label, used, remaining) in [
        ("on pace", 50.0, Some(302_400)),
        ("spare allowance", 25.0, Some(302_400)),
        ("allowance shortfall", 75.0, Some(302_400)),
        ("early overspending", 15.0, Some(565_200)),
        ("one hour left", 94.0, Some(3_600)),
        ("two minutes left", 94.0, Some(120)),
        ("fractional usage", 94.5, Some(120)),
        ("under one minute", 99.9, Some(30)),
        ("near reset on pace", 99.98015873015873, Some(120)),
        ("new window", 0.0, Some(604_800)),
        ("usage before elapsed time", 1.0, Some(604_800)),
        ("future window", 0.0, Some(604_860)),
        ("exhausted", 100.0, Some(120)),
        ("over limit", 101.0, Some(120)),
        ("reset now", 94.0, Some(0)),
        ("reset passed", 94.0, Some(-60)),
        ("unknown reset", 94.0, None),
    ] {
        let mut payload = payload();
        payload["rate_limits"] = json!({
            "seven_day": {
                "used_percentage": used,
                "resets_at": remaining.map(|seconds| NOW + seconds),
            },
        });
        let line = render_hook(payload).await;
        output.push_str(&format!("{label}: {line}\n"));
    }
    insta::assert_snapshot!(output);
}

#[tokio::test]
async fn pace_switches_to_multiplier_at_100_percent_deviation() {
    let mut output = String::new();
    for (label, used, remaining) in [
        ("over 99 percent", 39.8, 14_400),
        ("over 100 percent", 40.0, 14_400),
        ("over 101 percent", 40.2, 14_400),
        ("over 225 percent", 65.0, 14_400),
        ("over rounds to 99 percent", 39.88, 14_400),
        ("over rounds to 100 percent", 39.92, 14_400),
        ("under 99 percent", 60.2, 3_600),
        ("under 100 percent", 60.0, 3_600),
        ("under 101 percent", 59.8, 3_600),
        ("under 225 percent", 35.0, 3_600),
        ("under rounds to 99 percent", 60.12, 3_600),
        ("under rounds to 100 percent", 60.08, 3_600),
    ] {
        let mut payload = payload();
        payload["rate_limits"] = json!({
            "five_hour": {"used_percentage": used, "resets_at": NOW + remaining},
        });
        let line = render_hook(payload).await;
        output.push_str(&format!("{label}: {line}\n"));
    }
    insta::assert_snapshot!(output);
}

#[tokio::test]
async fn five_hour_pace_preserves_status_line_styles() {
    let mut payload = payload();
    payload["rate_limits"] = json!({
        "five_hour": {"used_percentage": 94.0, "resets_at": NOW + 120},
    });
    let line = render_hook(payload).await;
    insta::assert_debug_snapshot!(line);
}
