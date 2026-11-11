#!/usr/bin/env python3
"""Check that CI repeats the fixed local path dependency revisions."""

from __future__ import annotations

import argparse
import re
import shlex
import sys
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class Dependency:
    path: str
    url: str
    revision: str


def error(errors: list[str], path: Path, line: int, message: str) -> None:
    errors.append(f"{path}:{line}: {message}")


def read_manifest(root: Path, errors: list[str]) -> list[Dependency]:
    path = root / ".infra/ci/local-path-dependencies.tsv"
    dependencies: list[Dependency] = []
    seen: set[str] = set()
    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except OSError as exc:
        error(errors, path, 1, f"cannot read dependency manifest: {exc}")
        return dependencies
    for number, line in enumerate(lines, 1):
        if not line or line.startswith("#"):
            continue
        fields = line.split("\t")
        if len(fields) != 3:
            error(errors, path, number, "expected exactly path, URL, and full revision separated by tabs")
            continue
        dep = Dependency(*fields)
        if dep.path in seen:
            error(errors, path, number, f"duplicate dependency path {dep.path!r}")
        seen.add(dep.path)
        if dep.path not in ("../rs-redact", "../rs-json"):
            error(errors, path, number, f"unexpected dependency path {dep.path!r}")
        expected_url = {
            "../rs-redact": "https://github.com/qubit-ltd/rs-redact.git",
            "../rs-json": "https://github.com/qubit-ltd/rs-json.git",
        }.get(dep.path)
        if expected_url is not None and dep.url != expected_url:
            error(errors, path, number, f"{dep.path} must map to {expected_url}")
        if not re.fullmatch(r"https://github\.com/qubit-ltd/[a-z0-9-]+\.git", dep.url):
            error(errors, path, number, f"unexpected repository URL {dep.url!r}")
        if not re.fullmatch(r"[0-9a-fA-F]{40}", dep.revision):
            error(errors, path, number, "revision must be a full 40-character hexadecimal SHA")
        dependencies.append(dep)
    if {d.path for d in dependencies} != {"../rs-redact", "../rs-json"}:
        error(errors, path, 1, "manifest must contain exactly rs-redact and rs-json")
    return dependencies


def check_checkouts(root: Path, deps: list[Dependency], errors: list[str]) -> None:
    path = root / ".github/workflows/conditional-write.yml"
    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except OSError as exc:
        error(errors, path, 1, f"cannot read workflow: {exc}")
        return
    repos = {dep.url.removeprefix("https://github.com/").removesuffix(".git"): dep for dep in deps}
    required_jobs = {"windows-root-enumeration-fallback", "conditional", "windows-recovery"}
    jobs: dict[str, list[tuple[int, dict[str, object]]]] = {}
    current_job: str | None = None
    current_step: dict[str, object] | None = None
    step_indent = 0
    for index, line in enumerate(lines, 1):
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        indent = len(line) - len(line.lstrip())
        job_match = re.fullmatch(r"  ([A-Za-z0-9_-]+):\s*(?:#.*)?", line)
        if job_match:
            current_job = job_match.group(1)
            jobs.setdefault(current_job, [])
            current_step = None
            continue
        if current_job is None:
            continue
        if indent == 6 and re.match(r"-\s+", line.lstrip()):
            current_step = {
                "uses": [],
                "with_fields": {},
                "step_fields": {},
                "nested_fields": [],
                "with_indent": None,
                "with_count": 0,
            }
            step_indent = indent
            jobs[current_job].append((index, current_step))
            inline = re.match(r"\s*-\s+([A-Za-z][A-Za-z0-9_-]*):\s*(.*?)\s*$", line)
            if inline:
                key, value = inline.groups()
                if key == "uses":
                    current_step["uses"].append((index, _yaml_scalar(value)))
                else:
                    current_step["step_fields"].setdefault(key, []).append((index, _yaml_scalar(value)))
            continue
        if current_step is not None and indent > step_indent:
            field = re.match(r"\s+([A-Za-z][A-Za-z0-9_-]*):\s*(.*?)\s*$", line)
            if field:
                key, raw_value = field.groups()
                value = _yaml_scalar(raw_value)
                if indent == step_indent + 2:
                    if key != "with":
                        current_step["with_indent"] = None
                    if key == "uses":
                        current_step["uses"].append((index, value))
                    elif key == "with":
                        current_step["with_count"] += 1
                        current_step["with_indent"] = indent
                        if value:
                            current_step["nested_fields"].append((index, "with", value))
                    else:
                        current_step["step_fields"].setdefault(key, []).append((index, value))
                elif current_step["with_indent"] is not None and indent == current_step["with_indent"] + 2:
                    current_step["with_fields"].setdefault(key, []).append((index, value))
                elif current_step["with_indent"] is not None and indent > current_step["with_indent"] + 2:
                    current_step["nested_fields"].append((index, key, value))
                else:
                    current_step["nested_fields"].append((index, key, value))

    job_repositories: dict[str, set[str]] = {}
    for job, steps in jobs.items():
        for line_no, fields in steps:
            uses_values = fields["uses"]
            with_fields = fields["with_fields"]
            step_fields = fields["step_fields"]
            nested_fields = fields["nested_fields"]
            repo_values = with_fields.get("repository", []) + step_fields.get("repository", [])
            path_values = with_fields.get("path", []) + step_fields.get("path", [])
            related = any(
                value in repos or any(d.path.split("/")[-1] in value for d in deps)
                for _, value in repo_values + path_values
            ) or any(
                any(d.path.split("/")[-1] in value for d in deps)
                for _, _, value in nested_fields
            )
            if not uses_values and related:
                error(errors, path, line_no, "dependency checkout layout is not recognized")
                continue
            checkout = any(value.startswith("actions/checkout@") for _, value in uses_values)
            if related and not checkout:
                error(errors, path, line_no, "dependency checkout layout is not recognized")
                continue
            if not checkout:
                continue
            for dep in deps:
                repo = dep.url.removeprefix("https://github.com/").removesuffix(".git")
                expected_path = f"ci-root/{dep.path[3:]}"
                repo_basename = dep.path.split("/")[-1]
                is_related = (
                    repo in [v for _, v in repo_values]
                    or expected_path in [v for _, v in path_values]
                    or any(repo_basename in value for _, value in repo_values + path_values)
                    or any(repo_basename in value for _, _, value in nested_fields)
                )
                if not is_related:
                    continue
                job_repositories.setdefault(job, set()).add(repo)
                if fields["with_count"] != 1:
                    error(errors, path, line_no, f"checkout for {repo} must have exactly one with mapping")
                if nested_fields:
                    nested_line, nested_key, _ = nested_fields[0]
                    error(errors, path, nested_line, f"checkout for {repo} has unsupported nested or misplaced field {nested_key!r}")
                for field_name in ("uses", "repository", "ref", "path"):
                    values = uses_values if field_name == "uses" else with_fields.get(field_name, [])
                    if len(values) != 1:
                        error(errors, path, line_no, f"checkout for {repo} has repeated or missing {field_name} field")
                values = {"uses": uses_values[0][1]} if len(uses_values) == 1 else {}
                values.update({name: entries[0][1] for name, entries in with_fields.items() if len(entries) == 1})
                if values.get("uses", "").startswith("${{"):
                    error(errors, path, line_no, f"checkout for {repo} uses a dynamic action")
                if values.get("repository") != repo:
                    error(errors, path, line_no, f"checkout for {repo} repository must be exactly {repo}")
                if values.get("ref") != dep.revision:
                    error(errors, path, line_no, f"checkout for {repo} must use manifest revision {dep.revision}")
                if values.get("path") != expected_path:
                    error(errors, path, line_no, f"checkout for {repo} path must be exactly {expected_path}")
    expected = set(repos)
    for job in required_jobs:
        if job not in jobs:
            error(errors, path, 1, f"required dependency preparation job {job!r} is missing")
    for job in required_jobs | set(job_repositories):
        repositories = job_repositories.get(job, set())
        missing = expected - repositories
        if missing:
            line_no = next((n for n, _ in jobs.get(job, [])), 1)
            error(errors, path, line_no, f"dependency preparation job {job!r} lacks checkout(s): {', '.join(sorted(missing))}")


def _yaml_scalar(value: str) -> str:
    quote: str | None = None
    escaped = False
    for index, char in enumerate(value):
        if escaped:
            escaped = False
            continue
        if char == "\\" and quote == '"':
            escaped = True
            continue
        if quote is not None:
            if char == quote:
                quote = None
        elif char in "\"'":
            quote = char
        elif char == "#" and index > 0 and value[index - 1].isspace():
            value = value[:index]
            break
    value = value.strip()
    if len(value) >= 2 and value[0] == value[-1] and value[0] in "\"'":
        value = value[1:-1]
    return value


def check_freebsd(root: Path, deps: list[Dependency], errors: list[str]) -> None:
    path = root / ".github/workflows/ci.yml"
    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except OSError as exc:
        error(errors, path, 1, f"cannot read workflow: {exc}")
        return
    job_start = next((i for i, line in enumerate(lines) if re.fullmatch(r"  freebsd-ci:\s*", line)), None)
    if job_start is None:
        error(errors, path, 1, "FreeBSD dependency preparation job is missing")
        return
    job_end = next((i for i in range(job_start + 1, len(lines)) if re.match(r"^  [A-Za-z0-9_-]+:\s*$", lines[i])), len(lines))
    step_starts = [i for i in range(job_start + 1, job_end) if re.match(r"^      -\s+", lines[i])]
    steps: list[dict[str, object]] = []
    for position, start in enumerate(step_starts):
        end = step_starts[position + 1] if position + 1 < len(step_starts) else job_end
        uses: list[tuple[int, str]] = []
        with_count = 0
        with_active = False
        prepares: list[tuple[int, int, bool, list[tuple[int, str]]]] = []
        for i in range(start, end):
            raw = lines[i]
            indent = len(raw) - len(raw.lstrip())
            inline_uses = re.match(r"^\s*-\s+uses:\s*(.*?)\s*$", raw)
            if inline_uses:
                uses.append((i + 1, _yaml_scalar(inline_uses.group(1))))
            elif indent == 8:
                field = re.match(r"^\s{8}([A-Za-z][A-Za-z0-9_-]*):\s*(.*?)\s*$", raw)
                if field:
                    key, value = field.groups()
                    if key == "uses":
                        uses.append((i + 1, _yaml_scalar(value)))
                    with_active = key == "with" and not _yaml_scalar(value)
                    if key == "with":
                        with_count += 1
            prepare_match = re.match(r"^(\s*)prepare:\s*(.*?)\s*$", raw)
            if not prepare_match:
                continue
            prepare_indent = len(prepare_match.group(1))
            prepare_value = _yaml_scalar(prepare_match.group(2))
            valid_layout = prepare_indent == 10 and with_active and prepare_value == "|"
            body: list[tuple[int, str]] = []
            if prepare_value == "|":
                for body_index in range(i + 1, end):
                    body_raw = lines[body_index]
                    if body_raw.strip() and len(body_raw) - len(body_raw.lstrip()) <= prepare_indent:
                        break
                    body.append((body_index + 1, body_raw.strip()))
            prepares.append((i + 1, prepare_indent, valid_layout, body))
        steps.append({"start": start + 1, "uses": uses, "with_count": with_count, "prepares": prepares})

    vm_steps = [step for step in steps if any(value.startswith("vmactions/freebsd-vm@") for _, value in step["uses"])]
    if len(vm_steps) != 1:
        error(errors, path, job_start + 1, f"expected exactly one FreeBSD VM action, found {len(vm_steps)}")
    for step in steps:
        is_vm = step in vm_steps
        for prepare_line, _, valid_layout, body in step["prepares"]:
            mentions_dependency = any(dep.path in command for dep in deps for _, command in body)
            if mentions_dependency and (not is_vm or not valid_layout):
                error(errors, path, prepare_line, "dependency pins must be in the FreeBSD VM action's direct with.prepare block")
    if len(vm_steps) != 1:
        return
    vm_step = vm_steps[0]
    if len(vm_step["uses"]) != 1:
        error(errors, path, vm_step["start"], "FreeBSD VM action step must have exactly one uses field")
    if vm_step["with_count"] != 1:
        error(errors, path, vm_step["start"], "FreeBSD VM action must have exactly one with mapping")
    all_prepares = vm_step["prepares"]
    valid_prepares = [prepare for prepare in all_prepares if prepare[2]]
    if len(all_prepares) != 1:
        line_no = all_prepares[1][0] if len(all_prepares) > 1 else vm_step["start"]
        error(errors, path, line_no, f"FreeBSD VM action must have exactly one prepare declaration, found {len(all_prepares)}")
        return
    if len(valid_prepares) != 1:
        line_no = all_prepares[0][0] if all_prepares else vm_step["start"]
        error(errors, path, line_no, "FreeBSD VM action prepare must be a direct with.prepare: | block")
        return
    prepare_line, prepare_indent, _, prepare_body = valid_prepares[0]
    commands: dict[str, list[tuple[int, str, list[str]]]] = {d.path: [] for d in deps}
    for line_no, command in prepare_body:
        if not command or command.startswith("#"):
            continue
        for dep in deps:
            if dep.path not in command:
                continue
            if command in (f"test ! -e {dep.path}", f"git init {dep.path}"):
                continue
            if not (command.startswith("git -C ") or command.startswith('test "')):
                error(errors, path, line_no, f"FreeBSD {dep.path[3:]} related command has unrecognized format")
                continue
            if command.startswith("test "):
                commands[dep.path].append((line_no, command, []))
                continue
            try:
                argv = shlex.split(command, comments=True, posix=True)
            except ValueError as exc:
                error(errors, path, line_no, f"cannot parse FreeBSD command: {exc}")
                continue
            commands[dep.path].append((line_no, command, argv))
    for dep in deps:
        repo = dep.path[3:]
        checkout_path = dep.path
        items = commands[checkout_path]
        kinds: list[str] = []
        for line_no, command, argv in items:
            if command.startswith("test "):
                head = re.fullmatch(
                    r'test\s+"\$\(git -C (\S+) rev-parse HEAD\)"\s*=\s*(?:"([^"\s]+)"|\'([^\'\s]+)\'|([^\s#]+))(?:\s+#.*)?',
                    command,
                )
                if not head or head.group(1) != checkout_path:
                    error(errors, path, line_no, f"FreeBSD {repo} HEAD assertion has unrecognized format")
                    continue
                kinds.append("head")
                actual_revision = next(value for value in head.groups()[1:] if value is not None)
                if actual_revision != dep.revision:
                    error(errors, path, line_no, f"FreeBSD {repo} HEAD assertion differs from manifest")
                continue
            if len(argv) < 4 or argv[:2] != ["git", "-C"] or argv[2] != checkout_path:
                error(errors, path, line_no, f"FreeBSD {repo} git command has unrecognized format")
                continue
            args = argv[3:]
            if args[:3] == ["remote", "add", "origin"] and len(args) == 4:
                kinds.append("remote")
                if args[3] != dep.url:
                    error(errors, path, line_no, f"FreeBSD {repo} remote URL differs from manifest")
            elif args[:3] == ["fetch", "--depth", "1"] and len(args) == 5 and args[3] == "origin":
                kinds.append("fetch")
                if args[4] != dep.revision:
                    error(errors, path, line_no, f"FreeBSD {repo} fetch revision differs from manifest")
            elif args == ["checkout", "--detach", "FETCH_HEAD"]:
                kinds.append("checkout")
            else:
                error(errors, path, line_no, f"FreeBSD {repo} git command has unrecognized format")
        expected_kinds = ["remote", "fetch", "checkout", "head"]
        if kinds != expected_kinds:
            error(errors, path, items[0][0] if items else prepare_line + 1,
                  f"FreeBSD {repo} commands must be remote/fetch/checkout/HEAD exactly once and in order")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--project-root", type=Path, default=Path(__file__).resolve().parents[1])
    args = parser.parse_args()
    root = args.project_root.resolve()
    errors: list[str] = []
    dependencies = read_manifest(root, errors)
    check_checkouts(root, dependencies, errors)
    check_freebsd(root, dependencies, errors)
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print("CI dependency pins match .infra/ci/local-path-dependencies.tsv")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
