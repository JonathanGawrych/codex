use crate::RolloutConfig;
use crate::RolloutRecorder;
use crate::SortDirection;
use crate::ThreadSortKey;
use crate::ThreadsPage;
use chrono::TimeZone;
use chrono::Utc;
use codex_protocol::ThreadId;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::ThreadHistoryMode;
use codex_state::StateRuntime;
use codex_state::ThreadMetadata;
use codex_state::ThreadMetadataBuilder;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;

struct Fixture {
    _home: TempDir,
    config: RolloutConfig,
    state: Arc<StateRuntime>,
}

impl Fixture {
    async fn new() -> anyhow::Result<Self> {
        let home = TempDir::new()?;
        let config = RolloutConfig {
            codex_home: home.path().to_path_buf(),
            sqlite: codex_state::SqliteConfig::new_for_testing(home.path().abs()),
            cwd: home.path().to_path_buf(),
            model_provider_id: "test-provider".to_string(),
            generate_memories: false,
        };
        let state =
            StateRuntime::init(config.sqlite.clone(), config.model_provider_id.clone()).await?;
        state
            .mark_backfill_complete(/*last_watermark*/ None)
            .await?;
        Ok(Self {
            _home: home,
            config,
            state,
        })
    }

    async fn write_reverted_thread(&self, name: &str, day: u32) -> anyhow::Result<ThreadMetadata> {
        let thread_id = ThreadId::new();
        let old_cwd = self.config.codex_home.join("mac").join(name);
        let original_path =
            self.write_rollout(thread_id, &old_cwd, day, ThreadHistoryMode::Paginated)?;
        let active_path =
            self.write_rollout(thread_id, &old_cwd, day + 2, ThreadHistoryMode::Paginated)?;
        assert_ne!(original_path, active_path);
        let created_at = Utc
            .with_ymd_and_hms(2025, 1, day, 12, 0, 0)
            .single()
            .expect("valid date");
        let mut builder =
            ThreadMetadataBuilder::new(thread_id, active_path, created_at, SessionSource::Cli);
        builder.history_mode = ThreadHistoryMode::Paginated;
        builder.model_provider = Some(self.config.model_provider_id.clone());
        builder.cwd = self.config.codex_home.join("nas").join(name);
        let mut metadata = builder.build(&self.config.model_provider_id);
        metadata.first_user_message = Some("First prompt".to_string());
        metadata.preview = metadata.first_user_message.clone();
        metadata.name = Some(name.to_string());
        self.state.upsert_thread(&metadata).await?;
        Ok(metadata)
    }

    fn write_rollout(
        &self,
        thread_id: ThreadId,
        cwd: &Path,
        day: u32,
        history_mode: ThreadHistoryMode,
    ) -> anyhow::Result<PathBuf> {
        let folder = self
            .config
            .codex_home
            .join(format!("sessions/2025/01/{day:02}"));
        std::fs::create_dir_all(&folder)?;
        let rollout_id = ThreadId::new();
        let path = folder.join(format!(
            "rollout-2025-01-{day:02}T12-00-00-{thread_id}_{rollout_id}.jsonl"
        ));
        let records = [
            serde_json::json!({
                "timestamp": format!("2025-01-{day:02}T12:00:00Z"),
                "ordinal": 0,
                "type": "session_meta",
                "payload": {
                    "id": thread_id,
                    "session_id": thread_id,
                    "timestamp": format!("2025-01-{day:02}T12:00:00Z"),
                    "cwd": cwd,
                    "originator": "test",
                    "cli_version": "test",
                    "source": "cli",
                    "model_provider": self.config.model_provider_id,
                    "history_mode": history_mode,
                }
            }),
            serde_json::json!({
                "timestamp": format!("2025-01-{day:02}T12:00:01Z"),
                "ordinal": 1,
                "type": "event_msg",
                "payload": { "type": "user_message", "message": "First prompt", "kind": "plain" }
            }),
        ];
        let jsonl = records
            .iter()
            .map(serde_json::to_string)
            .collect::<Result<Vec<_>, _>>()?
            .join("\n");
        std::fs::write(&path, format!("{jsonl}\n"))?;
        Ok(path)
    }

    async fn assert_listing_matches_state(
        &self,
        cwd: Option<&Path>,
        sort: ThreadSortKey,
        direction: SortDirection,
    ) -> anyhow::Result<Vec<ThreadId>> {
        let cwd_filters = cwd.map(|path| vec![path.to_path_buf()]);
        let mut cursor = None;
        let mut ids = Vec::new();
        for _ in 0..10 {
            let expected = RolloutRecorder::list_threads_from_state_db(
                Some(self.state.clone()),
                &self.config,
                /*page_size*/ 1,
                cursor.as_ref(),
                sort,
                direction,
                &[SessionSource::Cli],
                /*model_providers*/ None,
                cwd_filters.as_deref(),
                &self.config.model_provider_id,
                /*search_term*/ None,
            )
            .await?;
            let actual = RolloutRecorder::list_threads(
                Some(self.state.clone()),
                &self.config,
                /*page_size*/ 1,
                cursor.as_ref(),
                sort,
                direction,
                &[SessionSource::Cli],
                /*model_providers*/ None,
                cwd_filters.as_deref(),
                &self.config.model_provider_id,
                /*search_term*/ None,
            )
            .await?;
            assert_eq!(actual, expected);
            ids.extend(
                actual
                    .items
                    .iter()
                    .map(|item| item.thread_id.expect("thread id")),
            );
            cursor = actual.next_cursor;
            if cursor.is_none() {
                return Ok(ids);
            }
        }
        anyhow::bail!("pagination did not finish")
    }
}

#[tokio::test]
async fn paginated_listing_returns_current_metadata_once_across_pages() -> anyhow::Result<()> {
    let fixture = Fixture::new().await?;
    let secretary = fixture
        .write_reverted_thread("secretary", /*day*/ 1)
        .await?;
    let tibbit = fixture.write_reverted_thread("tibbit", /*day*/ 2).await?;
    for sort in [
        ThreadSortKey::CreatedAt,
        ThreadSortKey::UpdatedAt,
        ThreadSortKey::RecencyAt,
    ] {
        for direction in [SortDirection::Asc, SortDirection::Desc] {
            let ids = fixture
                .assert_listing_matches_state(/*cwd*/ None, sort, direction)
                .await?;
            let expected = match direction {
                SortDirection::Asc => vec![secretary.id, tibbit.id],
                SortDirection::Desc => vec![tibbit.id, secretary.id],
            };
            assert_eq!(ids, expected);
        }
    }
    Ok(())
}

#[tokio::test]
async fn paginated_listing_filters_by_current_cwd_without_rewriting_metadata() -> anyhow::Result<()>
{
    let fixture = Fixture::new().await?;
    let thread = fixture
        .write_reverted_thread("secretary", /*day*/ 1)
        .await?;
    let original = std::fs::read(&thread.rollout_path)?;
    for cwd in [
        &thread.cwd,
        &fixture.config.codex_home.join("mac/secretary"),
    ] {
        for direction in [SortDirection::Asc, SortDirection::Desc] {
            let ids = fixture
                .assert_listing_matches_state(Some(cwd), ThreadSortKey::CreatedAt, direction)
                .await?;
            let expected = if cwd == &thread.cwd {
                vec![thread.id]
            } else {
                Vec::new()
            };
            assert_eq!(ids, expected);
        }
    }
    assert_eq!(std::fs::read(&thread.rollout_path)?, original);
    assert_eq!(fixture.state.get_thread(thread.id).await?, Some(thread));
    Ok(())
}

#[tokio::test]
async fn filtered_listing_still_discovers_unindexed_legacy_rollouts() -> anyhow::Result<()> {
    let fixture = Fixture::new().await?;
    let thread_id = ThreadId::new();
    let cwd = fixture.config.codex_home.join("legacy");
    let path = fixture.write_rollout(thread_id, &cwd, /*day*/ 1, ThreadHistoryMode::Legacy)?;
    let actual = RolloutRecorder::list_threads(
        Some(fixture.state.clone()),
        &fixture.config,
        /*page_size*/ 10,
        /*cursor*/ None,
        ThreadSortKey::CreatedAt,
        SortDirection::Desc,
        &[SessionSource::Cli],
        /*model_providers*/ None,
        Some(std::slice::from_ref(&cwd)),
        &fixture.config.model_provider_id,
        /*search_term*/ None,
    )
    .await?;
    let metadata = fixture
        .state
        .get_thread(thread_id)
        .await?
        .expect("discovered legacy thread");
    assert_eq!(
        (metadata.id, metadata.rollout_path, metadata.cwd),
        (thread_id, path, cwd)
    );
    let expected: ThreadsPage = RolloutRecorder::list_threads_from_state_db(
        Some(fixture.state.clone()),
        &fixture.config,
        /*page_size*/ 10,
        /*cursor*/ None,
        ThreadSortKey::CreatedAt,
        SortDirection::Desc,
        &[SessionSource::Cli],
        /*model_providers*/ None,
        /*cwd_filters*/ None,
        &fixture.config.model_provider_id,
        /*search_term*/ None,
    )
    .await?;
    assert_eq!(actual, expected);
    Ok(())
}

#[tokio::test]
async fn paginated_listing_searches_current_metadata_without_restoring_rollout_values()
-> anyhow::Result<()> {
    let fixture = Fixture::new().await?;
    let mut thread = fixture
        .write_reverted_thread("secretary", /*day*/ 1)
        .await?;
    thread.title = "Secretary on NAS".to_string();
    thread.model_provider = "current-provider".to_string();
    fixture.state.upsert_thread(&thread).await?;
    let thread = fixture
        .state
        .get_thread(thread.id)
        .await?
        .expect("updated metadata");
    let providers = [thread.model_provider.clone()];
    let page = RolloutRecorder::list_threads(
        Some(fixture.state.clone()),
        &fixture.config,
        /*page_size*/ 10,
        /*cursor*/ None,
        ThreadSortKey::CreatedAt,
        SortDirection::Desc,
        &[SessionSource::Cli],
        Some(&providers),
        Some(std::slice::from_ref(&thread.cwd)),
        &fixture.config.model_provider_id,
        Some("Secretary on NAS"),
    )
    .await?;
    let expected = RolloutRecorder::list_threads_from_state_db(
        Some(fixture.state.clone()),
        &fixture.config,
        /*page_size*/ 10,
        /*cursor*/ None,
        ThreadSortKey::CreatedAt,
        SortDirection::Desc,
        &[SessionSource::Cli],
        Some(&providers),
        Some(std::slice::from_ref(&thread.cwd)),
        &fixture.config.model_provider_id,
        Some("Secretary on NAS"),
    )
    .await?;
    assert_eq!(page, expected);
    assert_eq!(
        page.items
            .iter()
            .map(|item| item.thread_id)
            .collect::<Vec<_>>(),
        vec![Some(thread.id)]
    );
    assert_eq!(fixture.state.get_thread(thread.id).await?, Some(thread));
    Ok(())
}
