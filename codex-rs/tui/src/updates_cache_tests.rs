use super::*;
use crate::legacy_core::config::ConfigBuilder;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

#[tokio::test]
async fn dismiss_version_creates_cache_file_when_missing() {
    let codex_home = tempdir().expect("temp codex home");
    let config = ConfigBuilder::default()
        .codex_home(codex_home.path().to_path_buf())
        .build()
        .await
        .expect("load config");
    let version_file = version_filepath(&config);

    dismiss_version(&config, "999.0.0")
        .await
        .expect("dismiss version");

    let info = read_version_info(&version_file).expect("read version info");
    assert_eq!(info.last_checked_at, DateTime::<Utc>::UNIX_EPOCH);
    assert_eq!(
        (
            info.latest_version.as_str(),
            info.dismissed_version.as_deref()
        ),
        ("999.0.0", Some("999.0.0"))
    );
}

#[test]
fn cached_upgrade_version_is_returned_only_when_newer() {
    let cache_dir = tempdir().expect("temp cache directory");
    let version_file = cache_dir.path().join("version.json");
    let info = VersionInfo {
        latest_version: "0.150.1".to_string(),
        last_checked_at: Utc::now(),
        dismissed_version: None,
    };
    std::fs::write(
        &version_file,
        serde_json::to_string(&info).expect("serialize version info"),
    )
    .expect("write version info");

    assert_eq!(
        (
            read_cached_upgrade_version(&version_file, "0.149.1"),
            read_cached_upgrade_version(&version_file, "0.150.1"),
        ),
        (Some("0.150.1".to_string()), None)
    );
}
