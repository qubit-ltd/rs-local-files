#!/usr/bin/env python3
"""Check that CI repeats the fixed local path dependency revisions."""

from __future__ import annotations

import argparse
import re
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
    blocks: list[tuple[int, str | None, list[str]]] = []
    current_job: str | None = None
    for index, line in enumerate(lines):
        job_match = re.match(r"^  ([A-Za-z0-9_-]+):\s*$", line)
        if job_match:
            current_job = job_match.group(1)
        if re.match(r"\s*- uses: actions/checkout@", line):
            indent = len(line) - len(line.lstrip())
            end = index + 1
            while end < len(lines):
                current = lines[end]
                if current.strip() and len(current) - len(current.lstrip()) <= indent:
                    break
                end += 1
            blocks.append((index + 1, current_job, lines[index:end]))
    job_dependencies: dict[str, set[str]] = {}
    for dep in deps:
        repo = dep.url.removeprefix("https://github.com/").removesuffix(".git")
        for line_no, job, block in blocks:
            joined = "\n".join(block)
            if repo not in joined and dep.path.split("/")[-1] not in joined:
                continue
            if job is not None:
                job_dependencies.setdefault(job, set()).add(repo)
            if not re.search(rf"(?m)^\s+repository:\s*{re.escape(repo)}\s*$", joined):
                error(errors, path, line_no, f"checkout for {repo} lacks a recognized repository field")
            if not re.search(rf"(?m)^\s+ref:\s*{re.escape(dep.revision)}\s*$", joined):
                error(errors, path, line_no, f"checkout for {repo} must use manifest revision {dep.revision}")
            if not re.search(rf"(?m)^\s+path:\s*ci-root/{re.escape(dep.path[3:])}\s*$", joined):
                error(errors, path, line_no, f"checkout for {repo} has missing or unexpected path")
    expected_repositories = {
        dep.url.removeprefix("https://github.com/").removesuffix(".git") for dep in deps
    }
    if len(job_dependencies) < 3:
        error(errors, path, 1, f"expected at least three dependency preparation jobs, found {len(job_dependencies)}")
    for job, repositories in job_dependencies.items():
        missing = expected_repositories - repositories
        if missing:
            error(errors, path, 1, f"dependency preparation job {job!r} lacks checkout(s): {', '.join(sorted(missing))}")


def check_freebsd(root: Path, deps: list[Dependency], errors: list[str]) -> None:
    path = root / ".github/workflows/ci.yml"
    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except OSError as exc:
        error(errors, path, 1, f"cannot read workflow: {exc}")
        return
    text = "\n".join(lines)
    for dep in deps:
        repo = dep.path[3:]
        url = dep.url
        references = [(i, line) for i, line in enumerate(lines, 1) if dep.path in line]
        if not references:
            error(errors, path, 1, f"FreeBSD preparation block for {repo} is missing")
            continue
        for line_no, line in references:
            if "git -C" not in line:
                continue
            if "remote add origin" in line:
                if url not in line:
                    error(errors, path, line_no, f"FreeBSD {repo} remote URL differs from manifest")
            elif "fetch" in line:
                if dep.revision not in line:
                    error(errors, path, line_no, f"FreeBSD {repo} fetch revision differs from manifest")
            elif "rev-parse HEAD" in line:
                if dep.revision not in line:
                    error(errors, path, line_no, f"FreeBSD {repo} HEAD assertion differs from manifest")
        for phrase in ("remote add origin", "fetch --depth 1 origin", "rev-parse HEAD"):
            matching = [line for _, line in references if phrase in line]
            if not matching:
                error(errors, path, 1, f"FreeBSD {repo} preparation lacks {phrase}")
    if "freebsd-ci:" not in text:
        error(errors, path, 1, "FreeBSD dependency preparation job is missing")


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
