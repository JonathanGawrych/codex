use super::*;
use pretty_assertions::assert_eq;
use std::io::Cursor;

#[test]
fn confirmation_defaults_to_no_and_accepts_yes() -> io::Result<()> {
    let mut results = Vec::new();
    for response in ["", "\n", "n\n", "no\n", "y\n", "YES\n"] {
        let mut output = Vec::new();
        results.push(confirm_with_io(
            EmbeddedAppServerReason::ManagedDaemonUnavailable,
            &mut Cursor::new(response),
            &mut output,
        )?);
    }

    assert_eq!(results, vec![false, false, false, false, true, true]);
    Ok(())
}

#[test]
fn confirmation_prompt_explains_embedded_session_limitations() -> io::Result<()> {
    let mut output = Vec::new();
    let confirmed = confirm_with_io(
        EmbeddedAppServerReason::ManagedDaemonUnavailable,
        &mut Cursor::new("\n"),
        &mut output,
    )?;

    assert!(!confirmed);
    let output = std::str::from_utf8(&output).expect("prompt must be valid UTF-8");
    insta::assert_snapshot!(output, @r###"
    Codex cannot use the managed App Server because the managed App Server is not running or could not be reached.

    An embedded App Server keeps this session in this terminal. Remote Control and another TUI cannot open the session while it is running.

    Continue with an embedded App Server? [y/N] 
    "###);
    Ok(())
}
