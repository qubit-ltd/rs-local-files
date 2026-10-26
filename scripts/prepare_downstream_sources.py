#!/usr/bin/env python3
"""Prepare explicit coordinated checkouts without changing manifests or locks."""
import argparse
from collections import deque
import fnmatch
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
    scopes = [data, *data.get("target", {}).values(), data.get("workspace", {})]
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


def workspace_member_manifests(manifest: Path, root: Path):
    """Yield package manifests explicitly selected by this workspace."""
    data = manifest_data(manifest)
    workspace = data.get("workspace")
    if workspace is None:
        return
    excluded = workspace.get("exclude", [])
    for pattern in workspace.get("members", []):
        if not pattern or pattern == ".":
            continue
        for member in manifest.parent.glob(pattern):
            relative = member.relative_to(manifest.parent).as_posix()
            if any(fnmatch.fnmatchcase(relative, item) for item in excluded):
                continue
            candidate = member / "Cargo.toml" if member.is_dir() else member
            if candidate.is_file():
                yield inside(root, candidate)


def manifest_lockfile(manifest: Path, root: Path) -> Path:
    """Return the manifest's lockfile, including a workspace-owned lockfile."""
    lockfile = manifest.with_name("Cargo.lock")
    if lockfile.is_file():
        return lockfile
    data = manifest_data(manifest)
    if "workspace" in data or data.get("package", {}).get("workspace") is not None:
        return workspace_manifest(manifest, data, root).with_name("Cargo.lock")
    folder = manifest.parent
    while folder != root:
        candidate = folder / "Cargo.toml"
        if candidate.is_file() and "workspace" in manifest_data(candidate):
            return candidate.with_name("Cargo.lock")
        folder = folder.parent
    return lockfile


def local_package_dependencies(manifest: Path, root: Path, registry_names=frozenset()):
    """Return direct locked local qubit packages absent from the registry lock."""
    lockfile = manifest_lockfile(manifest, root)
    if not lockfile.is_file():
        return set()
    local_names = {
        package["name"]
        for package in manifest_data(lockfile).get("package", [])
        if package["name"].startswith("qubit-") and "source" not in package
    }
    data = manifest_data(manifest)
    scopes = [data, *data.get("target", {}).values(), data.get("workspace", {})]
    return {
        dependency.get("package", alias) if isinstance(dependency, dict) else alias
        for scope in scopes
        for table_name in DEPENDENCY_TABLES
        for alias, dependency in scope.get(table_name, {}).items()
        if (dependency.get("package", alias) if isinstance(dependency, dict) else alias) in local_names
        and (dependency.get("package", alias) if isinstance(dependency, dict) else alias) not in registry_names
    }


def use_latest_local_dependencies(manifest: Path, root: Path, checkout, registry_names=frozenset()) -> None:
    """Use latest sibling sources for locked qubit packages absent from the registry."""
    lockfile = manifest_lockfile(manifest, root)
    if not lockfile.is_file():
        return
    local_names = {
        package["name"]
        for package in manifest_data(lockfile).get("package", [])
        if package["name"].startswith("qubit-") and "source" not in package
    }
    data = manifest_data(manifest)
    scopes = [data, *data.get("target", {}).values(), data.get("workspace", {})]
    contents = manifest.read_text(encoding="utf-8")

    def update_declaration(source, table_name, alias, transform):
        """Update matching aliases only inside dependency table sections."""
        section = re.compile(
            rf"(?ms)(?P<header>^[ \t]*\[[^\]\r\n]*\.{re.escape(table_name)}\][ \t]*\r?\n|"
            rf"^[ \t]*\[{re.escape(table_name)}\][ \t]*\r?\n)"
            rf"(?P<body>.*?)(?=^[ \t]*\[|\Z)"
        )
        declaration = re.compile(
            rf"(?m)^(?P<prefix>[ \t]*{re.escape(alias)}[ \t]*=[ \t]*)"
            rf"(?P<value>[^\r\n]+)$"
        )

        def update_section(match):
            body, count = declaration.subn(transform, match.group("body"))
            return match.group("header") + body if count else match.group(0)

        updated, count = section.subn(update_section, source)
        if count == 0:
            raise RuntimeError(f"cannot locate {alias} declaration in [{table_name}] in {manifest}")
        return updated

    for scope in scopes:
        for table_name in DEPENDENCY_TABLES:
            for alias, dependency in scope.get(table_name, {}).items():
                package_name = dependency.get("package", alias) if isinstance(dependency, dict) else alias
                if package_name not in local_names:
                    continue
                if isinstance(dependency, dict) and dependency.get("workspace"):
                    continue
                if package_name in registry_names:
                    if isinstance(dependency, dict) and "path" in dependency:
                        def remove_path(match):
                            value = match.group("value").strip()
                            if not value.startswith("{") or not value.endswith("}"):
                                raise RuntimeError(f"unsupported dependency syntax for {package_name} in {manifest}")
                            fields = re.sub(r"\bpath\s*=\s*(['\"])[^'\"]*\1", "", value[1:-1], count=1)
                            fields = fields.strip().strip(",").strip()
                            return f'{match.group("prefix")}{{{fields}}}'

                        contents = update_declaration(contents, table_name, alias, remove_path)
                    continue
                repository = package_name.replace("qubit-", "rs-", 1)
                sibling = checkout(repository)
                version = dependency if isinstance(dependency, str) else dependency.get("version")
                if not version and not (isinstance(dependency, dict) and "path" in dependency):
                    raise RuntimeError(f"missing version constraint for local {package_name} in {manifest}")
                if isinstance(dependency, dict) and "path" in dependency:
                    current = inside(root, manifest.parent / dependency["path"] / "Cargo.toml")
                    if current.is_file():
                        actual_name = manifest_data(current).get("package", {}).get("name")
                        if actual_name != package_name:
                            raise RuntimeError(f"{package_name} path resolves to {actual_name!r} in {manifest}")
                        if current.resolve() == (sibling / "Cargo.toml").resolve():
                            continue
                def add_path(match):
                    value = match.group("value").strip()
                    relative = Path(os.path.relpath(sibling, manifest.parent)).as_posix()
                    if value.startswith(("'", '"')):
                        return (f'{match.group("prefix")}{{ path = "{relative}", '
                                f'version = {value} }}')
                    if value.startswith("{") and value.endswith("}"):
                        fields = value[1:-1].strip().rstrip(",")
                        fields = re.sub(r"\bpath\s*=\s*(['\"])[^'\"]*\1", "", fields, count=1)
                        fields = fields.strip().strip(",").strip()
                        suffix = f", {fields}" if fields else ""
                        return f'{match.group("prefix")}{{ path = "{relative}"{suffix} }}'
                    raise RuntimeError(f"unsupported dependency syntax for {package_name} in {manifest}")

                contents = update_declaration(contents, table_name, alias, add_path)
    if contents != manifest.read_text(encoding="utf-8"):
        manifest.write_text(contents, encoding="utf-8")

    updated = manifest_data(manifest)
    for scope in [updated, *updated.get("target", {}).values(), updated.get("workspace", {})]:
        for table_name in DEPENDENCY_TABLES:
            for alias, dependency in scope.get(table_name, {}).items():
                package_name = dependency.get("package", alias) if isinstance(dependency, dict) else alias
                if package_name not in local_names:
                    continue
                if package_name in registry_names:
                    continue
                if isinstance(dependency, dict) and dependency.get("workspace"):
                    continue
                if not isinstance(dependency, dict) or "path" not in dependency:
                    raise RuntimeError(f"{package_name} must use a sibling path in {manifest}")
                actual = inside(root, manifest.parent / dependency["path"] / "Cargo.toml")
                if manifest_data(actual).get("package", {}).get("name") != package_name:
                    raise RuntimeError(f"{package_name} path resolves to a different package in {manifest}")


def redirect_fixture_io_paths(fixture: Path, manifests: set[Path] | None = None) -> None:
    """Make every selected qubit-io edge use the same newest sibling checkout."""
    io_directory = fixture / "rs-io"
    if manifests is None:
        manifests = {fixture / name / "Cargo.toml"
                     for name in ("rs-fs-local", "rs-mime", "rs-local-files")}
    selected = set(manifests)
    for manifest in tuple(selected):
        data = manifest_data(manifest)
        if data.get("package", {}).get("workspace") is not None:
            selected.add(workspace_manifest(manifest, data, fixture))
        else:
            folder = manifest.parent
            while folder != fixture:
                ancestor = folder / "Cargo.toml"
                if ancestor.is_file() and "workspace" in manifest_data(ancestor):
                    selected.add(ancestor)
                    break
                folder = folder.parent

    for manifest in sorted(selected):
        original = manifest.read_text(encoding="utf-8")
        relative_io = Path(os.path.relpath(io_directory, manifest.parent)).as_posix()

        def redirect(match):
            value = match.group("value").strip()
            comment = match.group("comment") or ""
            if value.startswith(("'", '"')):
                return (f'{match.group("indent")}qubit-io = {{ path = "{relative_io}", '
                        f'version = {value} }}{comment}')
            if not (value.startswith("{") and value.endswith("}")):
                raise RuntimeError(f"unsupported qubit-io dependency syntax in {manifest}")
            dependency = tomllib.loads(f"dependency = {value}")["dependency"]
            if dependency.get("workspace"):
                return match.group(0)
            if "path" in dependency:
                body = re.sub(r"\bpath\s*=\s*(['\"])[^'\"]*\1",
                              f'path = "{relative_io}"', value, count=1)
            else:
                body = value.replace("{", f'{{ path = "{relative_io}",', 1)
            return f'{match.group("indent")}qubit-io = {body}{comment}'

        updated = re.sub(
            r"(?m)^(?P<indent>[ \t]*)qubit-io[ \t]*=[ \t]*(?P<value>[^\r\n]+?)(?P<comment>[ \t]+#.*)?[ \t]*$",
            redirect,
            original,
        )
        if updated != original:
            manifest.write_text(updated, encoding="utf-8")
        data = manifest_data(manifest)
        scopes = [data, *data.get("target", {}).values()]
        dependencies = [scope.get(table_name, {}).get("qubit-io")
                        for scope in scopes for table_name in DEPENDENCY_TABLES]
        workspace = data.get("workspace", {}).get("dependencies", {}).get("qubit-io")
        dependencies.append(workspace)
        for dependency in dependencies:
            if dependency is None or (isinstance(dependency, dict) and dependency.get("workspace")):
                continue
            if not isinstance(dependency, dict) or "path" not in dependency:
                raise RuntimeError(f"qubit-io is not a sibling path dependency in {manifest}")
            resolved = (manifest.parent / dependency["path"]).resolve()
            if resolved != io_directory.resolve():
                raise RuntimeError(f"qubit-io path does not use selected sibling checkout in {manifest}: {resolved}")


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
    seeds = dict.fromkeys(("rs-fs-local", "rs-mime", "rs-io",
                           *(name for name in refs if name != "support")))
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
        candidate_lockfile = destination / "Cargo.lock"
        registry_names = {
            package["name"]
            for package in (manifest_data(candidate_lockfile).get("package", []) if candidate_lockfile.is_file() else [])
            if package["name"].startswith("qubit-") and package.get("source", "").startswith("registry+")
        }
        for package_name in local_package_dependencies(manifest, root, registry_names):
            repository = package_name.replace("qubit-", "rs-", 1)
            dependency_checkout = checkout(repository)
            queue.append(dependency_checkout / "Cargo.toml")
        data = manifest_data(manifest)
        if "workspace" in data:
            queue.extend(workspace_member_manifests(manifest, root))
        elif data.get("package", {}).get("workspace") is not None:
            queue.append(workspace_manifest(manifest, data, root))
        else:
            folder = manifest.parent
            while folder != root:
                workspace_root = folder / "Cargo.toml"
                if workspace_root.is_file() and "workspace" in manifest_data(workspace_root):
                    queue.append(workspace_root)
                    break
                folder = folder.parent
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
    fixture_manifests = {fixture / manifest.relative_to(root) for manifest in visited}
    def fixture_checkout(repository):
        """Return the selected dependency source inside the disposable fixture."""
        dependency_manifest = fixture / repository / "Cargo.toml"
        if not dependency_manifest.is_file():
            raise RuntimeError(f"candidate fixture missing local dependency: {dependency_manifest}")
        return fixture / repository

    for manifest in sorted(fixture_manifests):
        use_latest_local_dependencies(manifest, fixture, fixture_checkout, registry_names)
    redirect_fixture_io_paths(fixture, fixture_manifests)
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
