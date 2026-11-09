import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "coverage-provenance.py"


class CoverageProvenanceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="coverage provenance ")
        self.root = Path(self.temp.name)
        (self.root / ".git").mkdir()
        (self.root / ".infra/coverage").mkdir(parents=True)
        (self.root / ".infra/coverage/coverage.json").write_text("{}", encoding="utf-8")
        (self.root / ".infra/tools").mkdir(parents=True)
        (self.root / ".infra/tools/defaults.toml").write_text('build_toolchain = "stable"\n', encoding="utf-8")
        subprocess.run(["git", "init", "-q", str(self.root)], check=True)
        subprocess.run(["git", "-C", str(self.root), "config", "user.email", "coverage@example.invalid"], check=True)
        subprocess.run(["git", "-C", str(self.root), "config", "user.name", "Coverage Test"], check=True)
        self.coverage = self.root / "fake coverage command.py"
        self.coverage.write_text(
            """#!/usr/bin/env python3
import os, pathlib, sys
root = pathlib.Path.cwd()
if os.environ.get('FAKE_COVERAGE_FAIL'):
    raise SystemExit(17)
(root / 'coverage-summary.md').write_text('## New coverage\\n')
(root / 'coverage.json').write_text('{\\"data\\":[{}]}')
(root / 'lcov.info').write_text('TN:\\n')
(root / 'coverage.txt').write_text('coverage report\\n')
(root / 'coverage-badge.json').write_text('{}')
(root / 'ci-summary.json').write_text('{}')
(root / 'target/llvm-cov/html').mkdir(parents=True, exist_ok=True)
(root / 'target/llvm-cov/html/index.html').write_text('html')
(root / 'target/llvm-cov/cobertura.xml').write_text('<coverage/>')
(root / 'target/infra/coverage').mkdir(parents=True, exist_ok=True)
(root / 'target/infra/coverage/raw.json').write_text('{\"data\":[{}]}')
""",
            encoding="utf-8",
        )
        subprocess.run(["git", "-C", str(self.root), "add", "."], check=True)
        subprocess.run(["git", "-C", str(self.root), "commit", "-qm", "baseline"], check=True)

    def tearDown(self):
        self.temp.cleanup()

    def run_provenance(self, extra_env=None):
        env = os.environ.copy()
        env["RLF_COVERAGE_COMMAND"] = str(self.coverage)
        env["RLF_COVERAGE_TEST_MODE"] = "1"
        if extra_env:
            env.update(extra_env)
        return subprocess.run(
            [sys.executable, str(SCRIPT), "--project-root", str(self.root)],
            text=True,
            capture_output=True,
            env=env,
            check=False,
        )

    def test_success_records_local_provenance_and_effective_configuration(self):
        result = self.run_provenance()
        self.assertEqual(result.returncode, 0, result.stderr)
        metadata = json.loads((self.root / "coverage-metadata.json").read_text())
        self.assertEqual(metadata["run"]["kind"], "local")
        self.assertEqual(metadata["coverage"]["features"], ["all-features"])
        self.assertEqual(metadata["coverage"]["scope"], "default-members")
        self.assertTrue(metadata["config_sha256"][".infra/coverage/coverage.json"])
        self.assertIn("## New coverage\n", (self.root / "coverage-summary.md").read_text())

    def test_coverage_failure_does_not_publish_success_summary(self):
        result = self.run_provenance({"FAKE_COVERAGE_FAIL": "1"})
        self.assertEqual(result.returncode, 17)
        self.assertFalse((self.root / "coverage-metadata.json").exists())
        self.assertFalse((self.root / "coverage-summary.md").exists())

    def test_old_summary_is_isolated_before_command_runs(self):
        (self.root / "coverage-summary.md").write_text("old success\n")
        result = self.run_provenance()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("## New coverage\n", (self.root / "coverage-summary.md").read_text())

    def test_missing_report_fails_and_does_not_publish_metadata(self):
        self.coverage.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
        result = self.run_provenance()
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.root / "coverage-metadata.json").exists())

    def test_unknown_or_invalid_configuration_fails_closed(self):
        (self.root / ".infra/coverage/coverage.json").write_text('{"mystery": true}', encoding="utf-8")
        result = self.run_provenance()
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.root / "coverage-metadata.json").exists())

    def test_paths_with_spaces_are_passed_without_shell_interpolation(self):
        result = self.run_provenance()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads((self.root / "coverage-metadata.json").read_text())["project_root"], str(self.root))

    def test_dirty_worktree_is_recorded(self):
        (self.root / "untracked file").write_text("dirty")
        result = self.run_provenance()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(json.loads((self.root / "coverage-metadata.json").read_text())["git"]["dirty"])

    def test_actions_run_url_is_recorded(self):
        result = self.run_provenance({
            "GITHUB_SERVER_URL": "https://github.example",
            "GITHUB_REPOSITORY": "org/repo",
            "GITHUB_RUN_ID": "123",
            "GITHUB_RUN_ATTEMPT": "2",
        })
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            json.loads((self.root / "coverage-metadata.json").read_text())["run"]["url"],
            "https://github.example/org/repo/actions/runs/123/attempts/2",
        )


if __name__ == "__main__":
    unittest.main()
