use super::*;

#[test]
fn completion_writes_and_consumes_only_the_target_thread_marker() {
    let checkout = initialized_checkout();
    let target_thread_id = ThreadId::new();
    let other_thread_id = ThreadId::new();

    complete_source_update_in(checkout.path(), target_thread_id).expect("complete source update");

    assert_eq!(
        take_source_update_completion(checkout.path(), other_thread_id),
        Ok(false)
    );
    assert_eq!(
        take_source_update_completion(checkout.path(), target_thread_id),
        Ok(true)
    );
    assert_eq!(
        take_source_update_completion(checkout.path(), target_thread_id),
        Ok(false)
    );
}

#[test]
fn completion_rejects_a_checkout_with_uncommitted_changes() {
    let checkout = initialized_checkout();
    fs::write(checkout.path().join("tracked.txt"), "changed\n").expect("modify tracked file");

    let error =
        validate_source_update(checkout.path()).expect_err("dirty checkout should be rejected");

    assert!(error.to_string().contains("uncommitted changes"));
}

#[test]
fn completion_rejects_a_checkout_before_the_latest_stable_release() {
    let checkout = initialized_checkout();
    run_git(checkout.path(), &["checkout", "-b", "new-release"]).expect("create release branch");
    fs::write(checkout.path().join("tracked.txt"), "new release\n").expect("modify release file");
    run_git(
        checkout.path(),
        &[
            "-c",
            "user.name=Codex Tests",
            "-c",
            "user.email=codex-tests@example.com",
            "commit",
            "-am",
            "new release",
        ],
    )
    .expect("commit release");
    run_git(checkout.path(), &["tag", "rust-v0.151.0"]).expect("tag new release");
    run_git(checkout.path(), &["checkout", "main"]).expect("restore customization branch");

    let error = validate_source_update(checkout.path())
        .expect_err("checkout before latest stable release should be rejected");

    assert!(
        error
            .to_string()
            .contains("current branch is not rebased onto rust-v0.151.0")
    );
}

fn initialized_checkout() -> tempfile::TempDir {
    let checkout = tempfile::tempdir().expect("temp checkout");
    run_git(checkout.path(), &["init", "--initial-branch=main"]).expect("initialize checkout");
    fs::write(checkout.path().join("tracked.txt"), "original\n").expect("write tracked file");
    run_git(checkout.path(), &["add", "tracked.txt"]).expect("stage tracked file");
    run_git(
        checkout.path(),
        &[
            "-c",
            "user.name=Codex Tests",
            "-c",
            "user.email=codex-tests@example.com",
            "commit",
            "-m",
            "initial",
        ],
    )
    .expect("commit tracked file");
    run_git(checkout.path(), &["tag", "rust-v0.150.1"]).expect("tag stable release");
    checkout
}
