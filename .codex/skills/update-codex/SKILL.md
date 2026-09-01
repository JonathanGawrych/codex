---
name: update-codex
description: Update this customized Codex source checkout to the newest exact stable Rust release, preserve local commits, verify and build it, then restart the initiating TUI. Use only for updating this Codex repository checkout.
---

# Update Codex

An explicit invocation authorizes the fetch, rebase, conflict resolution, required tests, release
build, and completion signal described below. Keep the work in the current top-level session so the
completion signal targets the TUI that requested the update.

1. Resolve the repository root with `git rev-parse --show-toplevel`. Require that it contains both
   `.codex/skills/update-codex/SKILL.md` and `codex-rs`; stop if it does not.
2. Read `CODEX_THREAD_ID` with `printenv CODEX_THREAD_ID`. Require a UUID and record its literal
   value for the final command.
3. Require that the current branch is `main`. Inspect the checkout and preserve every local
   customization.
4. Require a Git remote named `upstream` for `https://github.com/openai/codex.git`, then run
   `git fetch --prune --tags upstream`.
5. Find the newest exact `rust-vMAJOR.MINOR.PATCH` tag. Ignore tags containing `alpha`, `beta`,
   `rc`, or any other suffix.
6. Rebase the current branch onto that stable release tag. Resolve every conflict using the newer
   release structure while preserving local behavior. Continue until the rebase finishes. Do not
   abort the rebase or discard local commits.
7. Follow the repository's `AGENTS.md` instructions and run the tests required for the changed code.
8. From the repository root, run `just build-source --profile release`. This downloads and verifies
   the Codex-built V8 artifacts required by the code-mode host.
9. After the tests and release build finish, inspect `git diff -- codex-rs/Cargo.lock`. Release tags
   leave local workspace package versions at `0.0.0`, and Cargo rewrites them to the release
   version. If and only if every change is this generated workspace-package version replacement,
   run `git restore --source=HEAD -- codex-rs/Cargo.lock`. If any dependency, checksum, source, or
   other line changed, determine why and preserve every required change.
10. Require the build to have succeeded, the rebase to be finished, `main` to be checked out, the
    newest stable release tag to be an ancestor of `HEAD`, and
    `git status --porcelain --untracked-files=normal` to be empty. Then run
    `/Users/jonathan/.local/bin/codex source-update-complete <thread-id>`, replacing `<thread-id>`
    with the literal UUID recorded in step 2.

Do not run the completion command if any required step failed. The command validates the checkout
and signals only the initiating TUI. The standalone package is installed and the managed app-server
is restarted after that TUI shuts down.
