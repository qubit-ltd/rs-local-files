#!/usr/bin/env python3
"""Verify coordinated local-files consumers with their unchanged lock files."""
import argparse
import hashlib
from pathlib import Path
import subprocess

REPOSITORIES = ("rs-local-files", "rs-fs-local", "rs-mime")


def lock_digest(root: Path) -> dict[str, str]:
    """Snapshot required manifests and locks, rejecting an incomplete layout."""
    result = {}
    for name in REPOSITORIES:
        for required in ("Cargo.toml", "Cargo.lock"):
            path = root / name / required
            if not path.is_file():
                raise RuntimeError(f"missing required file: {path}")
            result[f"{name}/{required}"] = hashlib.sha256(path.read_bytes()).hexdigest()
    return result


def run(root: Path, metadata_only: bool) -> None:
    """Check all locked graphs before tests, and fail on any input drift."""
    before = lock_digest(root)
    try:
        for name in REPOSITORIES:
            subprocess.run(["cargo", "metadata", "--locked", "--format-version", "1"],
                           cwd=root / name, check=True, stdout=subprocess.DEVNULL)
        if not metadata_only:
            for name in REPOSITORIES:
                subprocess.run(["cargo", "test", "--locked", "--all-features", "--no-fail-fast"],
                               cwd=root / name, check=True)
    finally:
        if before != lock_digest(root):
            raise RuntimeError("manifest or lock changed during locked verification")


def main() -> None:
    """Parse an explicit sibling-checkout root and run the requested checks."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--metadata-only", action="store_true")
    args = parser.parse_args()
    run(args.root.resolve(strict=True), args.metadata_only)


if __name__ == "__main__":
    main()
