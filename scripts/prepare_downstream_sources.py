#!/usr/bin/env python3
"""Prepare explicit coordinated checkouts without changing manifests or locks."""
import argparse
from collections import deque
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tomllib

DEPENDENCY_TABLES = ("dependencies", "dev-dependencies", "build-dependencies")


def inside(root: Path, path: Path) -> Path:
    """Resolve a path and reject equality with, or escape from, the layout root."""
    resolved = path.resolve()
    if resolved == root or not resolved.is_relative_to(root):
        raise RuntimeError(f"dependency path outside layout: {path}")
    return resolved


def manifest_data(path: Path) -> dict:
    """Read an existing Cargo manifest, preserving TOML parse failures."""
    with path.open("rb") as file:
        return tomllib.load(file)


def workspace_manifest(manifest: Path, data: dict, root: Path) -> Path:
    """Find the enclosing or explicit Cargo workspace for inherited dependencies."""
    explicit = data.get("package", {}).get("workspace")
    if explicit is not None:
        candidate = inside(root, manifest.parent / explicit / "Cargo.toml")
        if "workspace" not in manifest_data(candidate):
            raise RuntimeError(f"explicit workspace is not a workspace: {candidate}")
        return candidate
    folder = manifest.parent
    while folder != root:
        candidate = folder / "Cargo.toml"
        if candidate.is_file() and "workspace" in manifest_data(candidate):
            return candidate
        folder = folder.parent
    raise RuntimeError(f"missing workspace for inherited dependency: {manifest}")


def path_dependencies(manifest: Path, root: Path):
    """Yield normal/dev/build and target-specific path dependencies, with inheritance."""
    manifest = inside(root, manifest)
    data = manifest_data(manifest)
    scopes = [data, *data.get("target", {}).values()]
    for scope in scopes:
        for table in DEPENDENCY_TABLES:
            for name, dependency in scope.get(table, {}).items():
                if not isinstance(dependency, dict):
                    continue
                base = manifest.parent
                if dependency.get("workspace"):
                    workspace = workspace_manifest(manifest, data, root)
                    shared = manifest_data(workspace)["workspace"].get("dependencies", {})
                    if name not in shared:
                        raise RuntimeError(f"unknown inherited dependency {name!r} in {manifest}")
                    dependency = shared[name]
                    base = workspace.parent
                if isinstance(dependency, dict) and "path" in dependency:
                    yield inside(root, base / dependency["path"] / "Cargo.toml")


def redirect_fixture_io_paths(fixture: Path) -> None:
    """Point explicit qubit-io paths in the disposable tree at its one checkout."""
    io_directory = fixture / "rs-io"
    manifests = set()
    for name in ("rs-fs-local", "rs-mime", "rs-local-files"):
        manifest = fixture / name / "Cargo.toml"
        if not manifest.is_file():
            raise RuntimeError(f"candidate fixture missing resolver manifest: {manifest}")
        manifests.add(manifest)
        folder = manifest.parent
        while folder != fixture:
            ancestor = folder / "Cargo.toml"
            if ancestor.is_file():
                data = manifest_data(ancestor)
                if "workspace" in data:
                    manifests.add(ancestor)
                    break
            folder = folder.parent
        data = manifest_data(manifest)
        if data.get("package", {}).get("workspace") is not None:
            manifests.add(workspace_manifest(manifest, data, fixture))

    for manifest in sorted(manifests):
        original = manifest.read_text(encoding="utf-8")
        relative_io = Path(os.path.relpath(io_directory, manifest.parent)).as_posix()

        def redirect(match):
            body = match.group("body")
            if not re.search(r"\bpath\s*=", body):
                return match.group(0)
            body = re.sub(r"(\bpath\s*=\s*)(['\"])[^'\"]*\2",
                          lambda path_match: f'{path_match.group(1)}"{relative_io}"', body, count=1)
            return match.group("start") + body + match.group("end")

        updated = re.sub(
            r"(?m)^(?P<start>[ \t]*qubit-io[ \t]*=[ \t]*\{)(?P<body>[^}]*)(?P<end>\})",
            redirect, original)
        if updated != original:
            manifest.write_text(updated, encoding="utf-8")
        data = manifest_data(manifest)
        scopes = [data, *data.get("target", {}).values()]
        for scope in scopes:
            for table_name in DEPENDENCY_TABLES:
                dependency = scope.get(table_name, {}).get("qubit-io")
                if isinstance(dependency, dict) and "path" in dependency:
                    resolved = (manifest.parent / dependency["path"]).resolve()
                    if resolved != io_directory.resolve():
                        raise RuntimeError(f"candidate fixture qubit-io path was not redirected: {manifest}: {resolved}")
        workspace = data.get("workspace", {}).get("dependencies", {}).get("qubit-io")
        if isinstance(workspace, dict) and "path" in workspace:
            resolved = (manifest.parent / workspace["path"]).resolve()
            if resolved != io_directory.resolve():
                raise RuntimeError(f"candidate fixture workspace qubit-io path was not redirected: {manifest}: {resolved}")


def write_candidate_config(fixture: Path) -> Path:
    """Patch registry sources to the matching fixture copies."""
    config = fixture / ".cargo" / "config.toml"
    config.parent.mkdir()
    config.write_text(
        '[patch.crates-io]\n'
        'qubit-local-files = { path = "rs-local-files" }\n'
        'qubit-io = { path = "rs-io" }\n', encoding="utf-8")
    return config


def git_output(folder: Path, *arguments: str) -> str:
    """Read a Git result; failed commands retain their normal failure status."""
    return subprocess.run(["git", "-C", str(folder), *arguments], check=True,
                          text=True, stdout=subprocess.PIPE).stdout.strip()


def ensure_checkout(root: Path, name: str, ref: str) -> Path:
    """Checkout one qubit-ltd rs-* repository, refusing dirty existing worktrees."""
    if not re.fullmatch(r"rs-[a-z0-9][a-z0-9-]*", name):
        raise RuntimeError(f"unsupported repository name: {name!r}")
    if not ref or ref.startswith("-") or any(char.isspace() or char == "\0" for char in ref):
        raise RuntimeError(f"invalid Git ref: {ref!r}")
    folder = inside(root, root / name)
    remote = f"https://github.com/qubit-ltd/{name}.git"
    if folder.exists():
        if git_output(folder, "status", "--porcelain", "--untracked-files=all"):
            raise RuntimeError(f"refusing dirty existing checkout: {folder}")
        if git_output(folder, "rev-parse", "--show-toplevel") != str(folder):
            raise RuntimeError(f"target is not its own Git checkout: {folder}")
        if git_output(folder, "remote", "get-url", "origin") != remote:
            raise RuntimeError(f"unexpected dependency remote: {folder}")
    else:
        subprocess.run(["git", "clone", "--filter=blob:none", "--no-checkout", "--", remote, str(folder)], check=True)
    subprocess.run(["git", "-C", str(folder), "fetch", "--depth=1", "origin", ref], check=True)
    subprocess.run(["git", "-C", str(folder), "checkout", "--detach", "FETCH_HEAD"], check=True)
    subprocess.run(["git", "-C", str(folder), "submodule", "update", "--init", "--recursive"], check=True)
    return folder


def prepare(root: Path, candidate: Path, refs: dict[str, str]) -> dict:
    """Copy or reuse the candidate, then traverse checked-out path dependencies."""
    root = root.resolve()
    candidate = candidate.resolve(strict=True)
    if root == Path(root.anchor):
        raise RuntimeError("layout root must be a dedicated directory")
    destination = root / "rs-local-files"
    if candidate != destination and (destination.is_relative_to(candidate) or candidate.is_relative_to(destination)):
        raise RuntimeError("candidate and destination overlap")
    if not (candidate / "Cargo.toml").is_file():
        raise RuntimeError(f"candidate has no Cargo manifest: {candidate}")
    revision = git_output(candidate, "rev-parse", "HEAD")
    dirty = bool(git_output(candidate, "status", "--porcelain", "--untracked-files=all"))
    root.mkdir(parents=True, exist_ok=True)
    if candidate != destination:
        if destination.exists():
            raise RuntimeError(f"candidate destination already exists: {destination}")
        shutil.copytree(candidate, destination, symlinks=True,
                        ignore=shutil.ignore_patterns(".git", "target", ".codegraph", "__pycache__"))
    records = {"rs-local-files": {"revision": revision, "candidate_dirty": dirty}}

    def checkout(name):
        """Materialize each repository once, recording the selected immutable HEAD."""
        if name in records:
            return root / name
        ref = refs.get(name, refs["support"])
        folder = ensure_checkout(root, name, ref)
        records[name] = {"ref": ref, "revision": git_output(folder, "rev-parse", "HEAD")}
        return folder

    queue = deque([destination / "Cargo.toml"])
    seeds = dict.fromkeys(("rs-fs-local", "rs-mime", "rs-io", *(name for name in refs if name != "support")))
    for name in seeds:
        queue.append(checkout(name) / "Cargo.toml")
    visited = set()
    while queue:
        manifest = inside(root, queue.popleft())
        name = manifest.relative_to(root).parts[0]
        if name not in records:
            checkout(name)
        if manifest in visited:
            continue
        if not manifest.is_file():
            raise RuntimeError(f"missing path dependency manifest after checkout: {manifest}")
        visited.add(manifest)
        queue.extend(path_dependencies(manifest, root))
    # Materialize a private tree for candidate resolution. The original checkout
    # layout above remains the immutable --locked/hash verification channel.
    fixture = root / "candidate-fixture"
    if fixture.exists():
        raise RuntimeError(f"candidate fixture already exists: {fixture}")
    fixture.mkdir()
    for name in records:
        source = root / name
        if not source.is_dir():
            continue
        shutil.copytree(source, fixture / name, symlinks=True,
                        ignore=shutil.ignore_patterns(".git", "target", ".codegraph", "__pycache__"))
    redirect_fixture_io_paths(fixture)
    config = write_candidate_config(fixture)
    io_manifest = fixture / "rs-io/Cargo.toml"
    if not io_manifest.is_file():
        raise RuntimeError(f"candidate fixture missing same-source qubit-io checkout: {io_manifest}")
    for consumer in ("rs-fs-local", "rs-mime"):
        subprocess.run(["cargo", "metadata", "--format-version", "1"], cwd=fixture / consumer,
                       check=True, stdout=subprocess.DEVNULL)
    records["candidate-fixture"] = {"path": str(fixture), "config": str(config)}
    return records


def main() -> None:
    """Prepare a dedicated CI layout and print exact checkout provenance."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--fs-local-ref", required=True)
    parser.add_argument("--mime-ref", required=True)
    parser.add_argument("--support-ref", required=True)
    parser.add_argument("--additional-ref", action="append", default=[], metavar="REPOSITORY=REF")
    args = parser.parse_args()
    if os.environ.get("CI", "").lower() not in ("true", "1"):
        parser.error("source layout is CI-only; use --root with check_downstream_contracts.py for existing local worktrees")
    refs = {"rs-fs-local": args.fs_local_ref, "rs-mime": args.mime_ref, "support": args.support_ref}
    additional = set()
    for assignment in args.additional_ref:
        name, separator, ref = assignment.partition("=")
        if (not separator or not ref or not re.fullmatch(r"rs-[a-z0-9][a-z0-9-]*", name)
                or name == "rs-local-files" or name in additional):
            parser.error(f"invalid or duplicate additional repository ref: {assignment!r}")
        refs[name] = ref
        additional.add(name)
    records = prepare(args.root, args.candidate, refs)
    print(json.dumps(records, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
