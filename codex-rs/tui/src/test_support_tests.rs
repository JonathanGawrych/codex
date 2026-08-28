use super::normalize_snapshot_times;
use pretty_assertions::assert_eq;

#[test]
fn snapshot_times_keep_the_same_alignment_across_hour_widths() {
    for time in ["1:00 AM", "9:59 PM", "10:00 PM", "12:59 AM"] {
        let input = format!("{time:>80}\n› prompt\n• done{time:>74}\n");
        let expected = format!("{:>80}\n› prompt\n• done{:>74}\n", "<TIME>", "<TIME>");
        assert_eq!(normalize_snapshot_times(&input), expected);
    }
}
