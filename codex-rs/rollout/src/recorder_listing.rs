//! Preserve lineage from the selected rollout while listing current SQLite metadata.

use crate::ThreadItem;
use crate::ThreadsPage;

pub(crate) fn restore_scanned_parent_ids(
    db_page: codex_state::ThreadsPage,
    scanned: &[ThreadItem],
) -> ThreadsPage {
    let mut page: ThreadsPage = db_page.into();
    for item in &mut page.items {
        if item.parent_thread_id.is_none()
            && let Some(source) = scanned.iter().find(|source| source.path == item.path)
        {
            // Review/compaction parents can exist in session metadata without a spawn edge.
            item.parent_thread_id = source.parent_thread_id;
        }
    }
    page
}
