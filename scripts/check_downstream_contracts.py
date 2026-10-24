#!/usr/bin/env python3
"""Verify unchanged consumer locks and an isolated candidate dependency graph."""
import argparse
import hashlib
import json
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


def reachable(metadata: dict, root_id: str) -> set[str]:
    """Return package IDs reachable from one resolved consumer root."""
    resolve = metadata.get("resolve")
    if not isinstance(resolve, dict):
        raise RuntimeError(f"consumer root {root_id}: metadata has no resolve graph")
    nodes = {node.get("id"): node for node in resolve.get("nodes", [])}
    if root_id not in nodes:
        raise RuntimeError(f"consumer root {root_id}: root is absent from resolve graph")
    seen, pending = set(), [root_id]
    while pending:
        package_id = pending.pop()
        if package_id in seen:
            continue
        seen.add(package_id)
        node = nodes.get(package_id)
        if node is None:
            raise RuntimeError(f"consumer root {root_id}: missing resolve node {package_id}")
        pending.extend(dep["pkg"] for dep in node.get("deps", []))
    return seen


def validate_candidate_metadata(metadata: dict, root_id: str, expected_paths: dict[str, Path]) -> None:
    """Validate reachable local-files and io package identities by package ID."""
    packages = {package.get("id"): package for package in metadata.get("packages", [])}
    if root_id not in packages:
        raise RuntimeError(f"consumer root {root_id}: root package metadata is missing")
    ids = reachable(metadata, root_id)
    for name, expected in expected_paths.items():
        expected = expected.resolve()
        found = []
        for package_id in ids:
            package = packages.get(package_id)
            if package is None or package.get("name") != name:
                continue
            manifest = Path(package["manifest_path"]).resolve()
            found.append((package_id, manifest))
        if not found:
            raise RuntimeError(f"consumer {root_id}: reachable {name} package missing; expected {expected}")
        wrong = [(package_id, path) for package_id, path in found if path != expected]
        if wrong:
            details = ", ".join(f"{package_id} at {path}" for package_id, path in wrong)
            raise RuntimeError(f"consumer {root_id}: {name} source identity mismatch; expected {expected}; found {details}")


def _metadata(folder: Path, locked: bool) -> dict:
    command = ["cargo", "metadata", "--format-version", "1"]
    if locked:
        command.append("--locked")
    result = subprocess.run(command, cwd=folder, check=True, text=True, stdout=subprocess.PIPE)
    return json.loads(result.stdout)


def run(root: Path, metadata_only: bool, candidate_fixture: Path | None = None) -> None:
    """Check original locked graphs and, when supplied, the isolated candidate graph."""
    before = lock_digest(root)
    try:
        for name in REPOSITORIES:
            subprocess.run(["cargo", "metadata", "--locked", "--format-version", "1"],
                           cwd=root / name, check=True, stdout=subprocess.DEVNULL)
        if candidate_fixture is not None:
            fixture = candidate_fixture.resolve(strict=True)
            expected = {"qubit-local-files": fixture / "rs-local-files/Cargo.toml",
                        "qubit-io": fixture / "rs-io/Cargo.toml"}
            for name in ("rs-fs-local", "rs-mime"):
                metadata = _metadata(fixture / name, locked=True)
                resolve = metadata.get("resolve")
                root_id = resolve.get("root") if isinstance(resolve, dict) else None
                if not root_id:
                    raise RuntimeError(f"consumer {name}: metadata has no resolve root")
                validate_candidate_metadata(metadata, root_id, expected)
        if not metadata_only:
            for name in REPOSITORIES:
                subprocess.run(["cargo", "test", "--locked", "--all-features", "--no-fail-fast"],
                               cwd=root / name, check=True)
            if candidate_fixture is not None:
                for name in ("rs-fs-local", "rs-mime"):
                    subprocess.run(["cargo", "test", "--locked", "--all-features", "--no-fail-fast"],
                                   cwd=fixture / name, check=True)
    finally:
        if before != lock_digest(root):
            raise RuntimeError("manifest or lock changed during locked verification")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--candidate-fixture", type=Path)
    parser.add_argument("--metadata-only", action="store_true")
    args = parser.parse_args()
    run(args.root.resolve(strict=True), args.metadata_only, args.candidate_fixture)


if __name__ == "__main__":
    main()
