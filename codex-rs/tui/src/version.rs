/// The current Codex CLI version as embedded at compile time.
#[cfg(not(test))]
pub const CODEX_CLI_VERSION: &str = env!("CARGO_PKG_VERSION");

// UI snapshots use the workspace development version so release tags render identically in tests.
#[cfg(test)]
pub const CODEX_CLI_VERSION: &str = "0.0.0";
