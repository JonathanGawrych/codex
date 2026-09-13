use super::ThreadEnvironmentSettings;
use crate::protocol::EnvironmentConfigState;
use crate::protocol::TurnEnvironmentSelection;
use codex_utils_path_uri::LegacyAppPathString;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn saved_environment_paths_round_trip_as_native_strings() {
    for (cwd, shared) in [
        ("/Volumes/code/finance", "/Volumes/code/shared"),
        (r"C:\Users\finance", r"C:\Users\shared"),
    ] {
        let selection = TurnEnvironmentSelection {
            environment_id: "secondary".into(),
            cwd: LegacyAppPathString::from_string(cwd).try_into().unwrap(),
            workspace_roots: vec![
                LegacyAppPathString::from_string(cwd).try_into().unwrap(),
                LegacyAppPathString::from_string(shared).try_into().unwrap(),
            ],
            config: EnvironmentConfigState::FromThread,
        };
        let saved = serde_json::to_value(ThreadEnvironmentSettings::from(&selection)).unwrap();
        assert_eq!(
            saved,
            json!({
                "environment_id": "secondary",
                "cwd": cwd,
                "workspace_roots": [cwd, shared],
                "config_source": "thread",
            })
        );
        let saved: ThreadEnvironmentSettings = serde_json::from_value(saved).unwrap();
        assert_eq!(
            TurnEnvironmentSelection::try_from(saved).unwrap(),
            selection
        );
    }
}

#[test]
fn saved_attachment_requires_its_owner_to_supply_configuration_again() {
    let selection = TurnEnvironmentSelection {
        environment_id: "secondary".into(),
        cwd: LegacyAppPathString::from_string("/workspace")
            .try_into()
            .unwrap(),
        workspace_roots: vec![],
        config: EnvironmentConfigState::Failed("configuration unavailable".into()),
    };
    let saved = ThreadEnvironmentSettings::from(&selection);
    assert_eq!(
        TurnEnvironmentSelection::try_from(saved).unwrap(),
        TurnEnvironmentSelection {
            config: EnvironmentConfigState::Pending,
            ..selection
        }
    );
}

#[test]
fn invalid_saved_paths_fail_instead_of_selecting_another_workspace() {
    let saved: ThreadEnvironmentSettings = serde_json::from_value(json!({
        "environment_id": "secondary",
        "cwd": "relative/finance",
        "workspace_roots": [],
        "config_source": "thread",
    }))
    .unwrap();
    assert!(TurnEnvironmentSelection::try_from(saved).is_err());
}
