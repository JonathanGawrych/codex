#[cfg(unix)]
use std::os::unix::fs::symlink;

#[cfg(unix)]
use pretty_assertions::assert_eq;
#[cfg(unix)]
use tempfile::TempDir;

#[cfg(unix)]
use super::ExecServerRuntimePaths;

#[cfg(unix)]
#[test]
fn codex_executable_path_resolves_symlink() -> std::io::Result<()> {
    let temp_dir = TempDir::new()?;
    let executable = temp_dir.path().join("codex-real");
    let launcher = temp_dir.path().join("codex");
    std::fs::write(&executable, [])?;
    symlink(&executable, &launcher)?;

    let runtime_paths = ExecServerRuntimePaths::new(launcher, None)?;

    assert_eq!(
        runtime_paths.codex_self_exe.as_path(),
        std::fs::canonicalize(executable)?
    );
    Ok(())
}
