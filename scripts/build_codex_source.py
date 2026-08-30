#!/usr/bin/env python3
"""Build source Codex binaries with verified Codex-built V8 artifacts."""

import argparse
import os
from pathlib import Path
import sys


REPO_ROOT = Path(__file__).resolve().parent.parent
os.environ.setdefault("CODEX_REPO_ROOT", str(REPO_ROOT))
sys.path.insert(0, str(REPO_ROOT / "scripts"))

from codex_package.cargo import build_source_binaries
from codex_package.targets import PACKAGE_VARIANTS
from codex_package.targets import TARGET_SPECS
from codex_package.targets import default_target


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Build the Codex CLI and code-mode host with verified Codex-built "
            "V8 artifacts."
        )
    )
    parser.add_argument(
        "--target",
        choices=sorted(TARGET_SPECS),
        default=default_target(),
        help="Rust target triple to build for the current source installation.",
    )
    parser.add_argument(
        "--profile",
        default="release",
        help="Cargo profile for the source binaries.",
    )
    parser.add_argument(
        "--cargo",
        default="cargo",
        help="Cargo executable to use.",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    outputs = build_source_binaries(
        TARGET_SPECS[args.target],
        PACKAGE_VARIANTS["codex"],
        cargo=args.cargo,
        profile=args.profile,
        entrypoint_bin=None,
        code_mode_host_bin=None,
        bwrap_bin=None,
        codex_command_runner_bin=None,
        codex_windows_sandbox_setup_bin=None,
    )
    print(f"Built Codex CLI at {outputs.entrypoint_bin}")
    print(f"Built code-mode host at {outputs.code_mode_host_bin}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
