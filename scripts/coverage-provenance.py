#!/usr/bin/env python3
"""Run the repository coverage entry point and attach same-run provenance."""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
import platform
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any

OUTPUTS = (
    "coverage-summary.md",
    "coverage-metadata.json",
    "coverage.json",
    "lcov.info",
    "coverage.txt",
    "coverage-badge.json",
    "ci-summary.json",
    "target/llvm-cov",
    "target/infra/coverage/raw.json",
)
CONFIG_FILES = (
    ".infra/coverage/coverage.json",
    ".infra/ci/coverage.json",
    ".infra/tools/defaults.toml",
    ".infra/bootstrap-source.json",
)
DEFAULT_THRESHOLDS = {"lines": {"minimum": 90, "inclusive": False},
                      "functions": {"minimum": 95, "inclusive": True},
                      "regions": {"minimum": 85, "inclusive": False}}
ALLOWED_CONFIG = {"scope", "exclude_packages", "source_dirs", "threshold_exempt_files",
                  "thresholds", "clippy", "coverage_cfg_clippy", "run_coverage_cfg_clippy"}


def run(command: list[str], *, cwd: Path, env: dict[str, str] | None = None,
        capture: bool = True) -> subprocess.CompletedProcess[str]:
    return subprocess.run(command, cwd=cwd, env=env, text=True, check=False,
                          stdout=subprocess.PIPE if capture else None,
                          stderr=subprocess.PIPE if capture else None)


def output(command: list[str], cwd: Path) -> str | None:
    try:
        result = run(command, cwd=cwd)
    except OSError:
        return None
    return result.stdout.strip() if result.returncode == 0 else None


def git_info(root: Path) -> dict[str, Any]:
    head = output(["git", "rev-parse", "HEAD"], root)
    status = output(["git", "status", "--porcelain", "--untracked-files=all"], root)
    if head is None or status is None:
        raise RuntimeError("could not read Git HEAD and dirty state")
    return {"head": head, "dirty": bool(status)}


def load_configuration(root: Path, *, test_mode: bool) -> tuple[dict[str, Any], dict[str, str | None]]:
    config_path = root / ".infra/coverage/coverage.json"
    if config_path.exists():
        try:
            config = json.loads(config_path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as error:
            raise RuntimeError(f"coverage configuration is unreadable: {error}") from error
        if not isinstance(config, dict) or set(config) - ALLOWED_CONFIG:
            raise RuntimeError("coverage configuration has unknown or invalid fields")
    else:
        config = {}
    scope = config.get("scope", "default-members")
    if scope not in {"default-members", "workspace", "package"}:
        raise RuntimeError(f"unknown coverage scope: {scope!r}")
    excluded = config.get("exclude_packages", [])
    if not isinstance(excluded, list) or any(not isinstance(item, str) or not item for item in excluded):
        raise RuntimeError("coverage exclude_packages is invalid")
    thresholds_config = config.get("thresholds", {})
    if not isinstance(thresholds_config, dict) or set(thresholds_config) - {"lines", "functions", "regions", "branches"}:
        raise RuntimeError("coverage thresholds are unknown or invalid")
    thresholds = dict(DEFAULT_THRESHOLDS)
    for metric in DEFAULT_THRESHOLDS:
        if metric in thresholds_config:
            value = thresholds_config[metric]
            if not isinstance(value, (int, float)) or isinstance(value, bool) or not 0 <= value <= 100:
                raise RuntimeError(f"invalid {metric} coverage threshold")
            thresholds[metric] = {"minimum": value, "inclusive": metric == "functions"}
    if "branches" in thresholds_config:
        value = thresholds_config["branches"]
        if not isinstance(value, (int, float)) or isinstance(value, bool) or not 0 <= value <= 100:
            raise RuntimeError("invalid branches coverage threshold")
        thresholds["branches"] = {"minimum": value, "inclusive": True}
    report_config_path = root / ".infra/ci/coverage.json"
    if report_config_path.is_file():
        try:
            report_config = json.loads(report_config_path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as error:
            raise RuntimeError(f"report configuration is unreadable: {error}") from error
        if not isinstance(report_config, dict) or set(report_config) - {"scope", "exclude_packages"}:
            raise RuntimeError("report configuration has unknown or invalid fields")
    else:
        report_config = {}
    report_scope = report_config.get("scope", "default-members")
    report_excluded = report_config.get("exclude_packages", [])
    if report_scope not in {"default-members", "workspace", "package"}:
        raise RuntimeError(f"unknown report scope: {report_scope!r}")
    if not isinstance(report_excluded, list) or any(not isinstance(item, str) or not item for item in report_excluded):
        raise RuntimeError("report exclude_packages is invalid")
    if test_mode:
        selected_packages: list[str] = []
        unselected_packages: list[str] = []
        selected_report_packages: list[str] = []
    else:
        metadata_result = run(["cargo", "metadata", "--no-deps", "--format-version", "1",
                               "--manifest-path", str(root / "Cargo.toml")], cwd=root)
        if metadata_result.returncode:
            raise RuntimeError("cargo metadata failed while resolving effective coverage packages")
        try:
            cargo_metadata = json.loads(metadata_result.stdout)
            member_ids = set(cargo_metadata["workspace_members"])
            default_ids = set(cargo_metadata["workspace_default_members"])
            manifest_path = (root / "Cargo.toml").resolve()
            packages = [package for package in cargo_metadata["packages"]
                        if package["id"] in member_ids]
            def is_selected(package: dict[str, Any]) -> bool:
                package_manifest = Path(package["manifest_path"]).resolve()
                return ((scope == "workspace") or
                        (scope == "default-members" and package["id"] in default_ids) or
                        (scope == "package" and package_manifest == manifest_path))
            package_names = {package["name"] for package in packages}
            if any(name not in package_names for name in excluded):
                raise RuntimeError("coverage config excludes an unknown workspace package")
            selected_packages = sorted(package["name"] for package in packages
                                       if is_selected(package) and package["name"] not in excluded)
            unselected_packages = sorted(package_names - set(selected_packages))
            if any(name not in package_names for name in report_excluded):
                raise RuntimeError("report config excludes an unknown workspace package")
            selected_report_packages = sorted(
                package["name"] for package in packages
                if ((report_scope == "workspace") or
                    (report_scope == "default-members" and package["id"] in default_ids) or
                    (report_scope == "package" and Path(package["manifest_path"]).resolve() == manifest_path))
                and package["name"] not in report_excluded
            )
            if not selected_packages:
                raise RuntimeError("effective coverage package selection is empty")
        except (KeyError, TypeError, json.JSONDecodeError) as error:
            raise RuntimeError(f"could not resolve effective coverage packages: {error}") from error
    collection_args = (["--package", selected_packages[0]] if scope == "package" and len(selected_packages) == 1
                       else ([argument for package in selected_packages for argument in ("--package", package)]
                             if scope == "package" else ["--workspace"] +
                             [argument for package in unselected_packages for argument in ("--exclude", package)]))
    effective = {
        "config_path": ".infra/coverage/coverage.json",
        "scope": scope,
        "features": ["all-features"],
        "exclude_packages": excluded,
        "source_dirs": config.get("source_dirs", {}),
        "unconfigured_source_dirs_default": ["src"],
        "threshold_exempt_files": config.get("threshold_exempt_files", {}),
        "thresholds": thresholds,
        "collector_output": "target/infra/coverage/raw.json",
        "selected_packages": selected_packages,
        "unselected_packages": unselected_packages,
        "collection_argv": ["cargo", "llvm-cov", *collection_args, "--all-features", "--json",
                            "--output-path", "target/infra/coverage/raw.json"],
        "report_config_path": ".infra/ci/coverage.json",
        "report_scope": report_scope,
        "report_exclude_packages": report_excluded,
        "report_selected_packages": selected_report_packages,
    }
    digests: dict[str, str | None] = {}
    for relative in CONFIG_FILES:
        path = root / relative
        digests[relative] = hashlib.sha256(path.read_bytes()).hexdigest() if path.is_file() else None
    return effective, digests


def artifact_hashes(root: Path, required: list[str], log_path: Path) -> dict[str, str]:
    paths = {relative for relative in required if (root / relative).is_file()}
    html_root = root / "target/llvm-cov/html"
    paths.update(path.relative_to(root).as_posix() for path in html_root.rglob("*") if path.is_file())
    paths.add(log_path.relative_to(root).as_posix())
    digests: dict[str, str] = {}
    for relative in sorted(paths):
        digest = hashlib.sha256()
        with (root / relative).open("rb") as artifact:
            for chunk in iter(lambda: artifact.read(1024 * 1024), b""):
                digest.update(chunk)
        digests[relative] = digest.hexdigest()
    return digests


def configured_build_toolchain(root: Path) -> str:
    defaults = root / ".infra/tools/defaults.toml"
    try:
        for line in defaults.read_text(encoding="utf-8").splitlines():
            key, separator, value = line.partition("=")
            if separator and key.strip() == "build_toolchain":
                toolchain = value.strip().strip('"')
                if toolchain:
                    return toolchain
    except OSError as error:
        raise RuntimeError(f"could not read configured build toolchain: {error}") from error
    raise RuntimeError("build_toolchain is missing from .infra/tools/defaults.toml")


def run_url(env: dict[str, str]) -> dict[str, str]:
    server, repo, run_id = (env.get("GITHUB_SERVER_URL"), env.get("GITHUB_REPOSITORY"),
                            env.get("GITHUB_RUN_ID"))
    if server and repo and run_id:
        url = f"{server.rstrip('/')}/{repo}/actions/runs/{run_id}"
        if env.get("GITHUB_RUN_ATTEMPT"):
            url += f"/attempts/{env['GITHUB_RUN_ATTEMPT']}"
        return {"kind": "github-actions", "url": url}
    return {"kind": "local", "url": "local"}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--project-root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("coverage_args", nargs=argparse.REMAINDER)
    options = parser.parse_args()
    root = options.project_root.resolve()
    if options.coverage_args and options.coverage_args[0] == "--":
        options.coverage_args = options.coverage_args[1:]
    test_mode = os.environ.get("RLF_COVERAGE_TEST_MODE") == "1"
    coverage_command = os.environ.get("RLF_COVERAGE_COMMAND") if test_mode else None
    command = ([sys.executable, coverage_command] if coverage_command else
               [str(root / ".infra/bin/coverage.sh")]) + options.coverage_args
    log_path = root / "coverage-provenance.log"
    try:
        effective, config_digests = load_configuration(root, test_mode=test_mode)
        git = git_info(root)
    except (OSError, RuntimeError) as error:
        print(f"coverage provenance: {error}", file=sys.stderr)
        return 2

    root.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="coverage-provenance-") as temporary:
        backup = Path(temporary)
        existing: list[str] = []
        for relative in OUTPUTS:
            original = root / relative
            if original.exists():
                destination = backup / relative
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.move(str(original), str(destination))
                existing.append(relative)
        try:
            build_toolchain = configured_build_toolchain(root)
            rustc = output(["rustc", "-vV"], root) if not shutil.which("rustup") else None
            if shutil.which("rustup"):
                rustc = output(["rustup", "run", build_toolchain, "rustc", "-vV"], root)
                cargo = output(["rustup", "run", build_toolchain, "cargo", "--version"], root)
                llvm = output(["rustup", "run", build_toolchain, "cargo", "llvm-cov", "--version"], root)
            else:
                cargo = output(["cargo", "--version"], root)
                llvm = output(["cargo", "llvm-cov", "--version"], root)
            env = os.environ.copy()
            if test_mode:
                env.pop("GITHUB_STEP_SUMMARY", None)
            with log_path.open("w", encoding="utf-8") as log:
                result = subprocess.run(command, cwd=root, env=env, text=True,
                                        stdout=log, stderr=subprocess.STDOUT, check=False)
            if result.returncode:
                raise RuntimeError(f"coverage command failed with exit code {result.returncode}")
            required = ["coverage-summary.md", "coverage.json", "lcov.info", "coverage.txt",
                        "coverage-badge.json", "ci-summary.json", "target/llvm-cov/html/index.html",
                        "target/llvm-cov/cobertura.xml", "target/infra/coverage/raw.json"]
            missing = [path for path in required if not (root / path).is_file()]
            if missing:
                raise RuntimeError("coverage reports missing: " + ", ".join(missing))
            host = next((line.removeprefix("host: ") for line in (rustc or "").splitlines()
                         if line.startswith("host: ")), platform.machine())
            with (root / "coverage-summary.md").open("a", encoding="utf-8") as summary:
                summary.write("\n- Provenance: `coverage-metadata.json` (same-run metadata).\n")
            hashes = artifact_hashes(root, required, log_path)
            metadata = {
                "schema": 1,
                "generated_at_utc": dt.datetime.now(dt.timezone.utc).isoformat().replace("+00:00", "Z"),
                "project_root": str(root),
                "git": git,
                "host": {"platform": platform.platform(), "triple": host, "target": os.environ.get("CARGO_BUILD_TARGET", host)},
                "tools": {"build_toolchain": build_toolchain,
                          "rustc_verbose_version": rustc or "unavailable",
                          "cargo_version": cargo or "unavailable",
                          "cargo_llvm_cov_version": llvm or "unavailable"},
                "coverage": effective,
                "config_sha256": config_digests,
                "entrypoint": {"argv": command, "path": ".infra/bin/coverage.sh"},
                "run": run_url(env),
                "artifacts": sorted(hashes),
                "artifact_sha256": hashes,
                "log": "coverage-provenance.log",
            }
            (root / "coverage-metadata.json").write_text(
                json.dumps(metadata, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        except (OSError, RuntimeError) as error:
            for relative in OUTPUTS:
                path = root / relative
                if path.is_dir():
                    shutil.rmtree(path)
                elif path.exists():
                    path.unlink()
            for relative in existing:
                source = backup / relative
                destination = root / relative
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.move(str(source), str(destination))
            (root / "coverage-metadata.json").unlink(missing_ok=True)
            print(f"coverage provenance: {error}; see {log_path}", file=sys.stderr)
            return result.returncode if "result" in locals() and result.returncode else 1
    print(f"coverage artifacts and provenance written under {root}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
