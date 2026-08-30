use pretty_assertions::assert_eq;

use super::executable_identity_from_bytes;
use super::parse_codex_version;
use super::paths_resolve_to_same_file;

#[test]
fn parses_codex_cli_version_output() {
    assert_eq!(
        parse_codex_version("codex 1.2.3\n").expect("version"),
        "1.2.3"
    );
}

#[test]
fn rejects_malformed_codex_cli_version_output() {
    assert!(parse_codex_version("codex\n").is_err());
}

#[test]
fn executable_identity_uses_binary_contents() {
    let old = executable_identity_from_bytes(b"old");
    let same = executable_identity_from_bytes(b"old");
    let new = executable_identity_from_bytes(b"new");

    assert_eq!(old, same);
    assert_ne!(old, new);
}

#[test]
fn detects_two_paths_that_resolve_to_the_same_executable() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let executable = temp_dir.path().join("codex");
    std::fs::write(&executable, []).expect("write executable");

    assert!(paths_resolve_to_same_file(&executable, &executable));
    assert!(!paths_resolve_to_same_file(
        &executable,
        &temp_dir.path().join("other-codex")
    ));
}
