#!/usr/bin/env python3
"""Keep FreeBSD sibling-checkout pins aligned with the shared TSV."""

from __future__ import annotations

import argparse
import re
from pathlib import Path


class PinError(ValueError):
    pass


def _tsv_pins(source: str) -> dict[str, tuple[str, str]]:
    pins: dict[str, tuple[str, str]] = {}
    for line_number, line in enumerate(source.splitlines(), 1):
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        fields = line.split("\t")
        if len(fields) != 3:
            raise PinError(f"TSV line {line_number}: expected three tab-separated fields")
        path, url, revision = fields
        if path in pins:
            raise PinError(f"TSV line {line_number}: duplicate dependency path {path}")
        pins[path] = (url, revision)
    return pins


def _workflow_pins(source: str, path: str) -> tuple[list[str], list[str], list[str]]:
    quoted = re.escape(path)
    remotes = re.findall(
        rf"git\s+-C\s+{quoted}\s+remote\s+add\s+origin\s+(\S+)\s*$",
        source,
        re.MULTILINE,
    )
    fetches = re.findall(
        rf"git\s+-C\s+{quoted}\s+fetch\s+[^\n]*?\borigin\s+([0-9a-f]{{40}})\s*$",
        source,
        re.MULTILINE,
    )
    heads = re.findall(
        rf"test\s+\"\$\(git\s+-C\s+{quoted}\s+rev-parse\s+HEAD\)\"\s*=\s*([0-9a-f]{{40}})\s*$",
        source,
        re.MULTILINE,
    )
    return remotes, fetches, heads


def validate(tsv: str, workflow: str) -> None:
    pins = _tsv_pins(tsv)
    expected = {"../rs-redact", "../rs-json"}
    if set(pins) & expected != expected:
        missing = sorted(expected - set(pins))
        raise PinError(f"TSV is missing required FreeBSD dependencies: {', '.join(missing)}")

    for path in sorted(expected):
        url, revision = pins[path]
        remotes, fetches, heads = _workflow_pins(workflow, path)
        if len(remotes) != 1:
            raise PinError(f"{path}: expected exactly one FreeBSD remote URL, found {len(remotes)}")
        if len(fetches) != 1:
            raise PinError(f"{path}: expected exactly one FreeBSD fetch pin, found {len(fetches)}")
        if len(heads) != 1:
            raise PinError(f"{path}: expected exactly one FreeBSD HEAD assertion, found {len(heads)}")
        if remotes[0] != url:
            raise PinError(f"{path}: FreeBSD remote URL {remotes[0]} does not match TSV {url}")
        if fetches[0] != revision:
            raise PinError(f"{path}: FreeBSD fetch pin {fetches[0]} does not match TSV {revision}")
        if heads[0] != revision:
            raise PinError(f"{path}: FreeBSD HEAD assertion {heads[0]} does not match TSV {revision}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--project-root", type=Path, default=Path(__file__).resolve().parents[1])
    args = parser.parse_args()
    root = args.project_root
    try:
        validate(
            (root / ".infra/ci/local-path-dependencies.tsv").read_text(encoding="utf-8"),
            (root / ".github/workflows/ci.yml").read_text(encoding="utf-8"),
        )
    except (OSError, PinError) as error:
        parser.error(str(error))
    print("FreeBSD dependency pins match .infra/ci/local-path-dependencies.tsv")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
