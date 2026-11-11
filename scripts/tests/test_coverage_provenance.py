import hashlib
import importlib.util
import io
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from contextlib import redirect_stderr
from unittest import mock
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "coverage-provenance.py"


def load_script():
    spec = importlib.util.spec_from_file_location("coverage_provenance_under_test", SCRIPT)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


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

    def run_inline(self, *, replace_side_effect=None, collector_side_effect=None,
                   rmtree_side_effect=None):
        module = load_script()
        real_rmtree = shutil.rmtree
        stderr = io.StringIO()
        env = {"RLF_COVERAGE_TEST_MODE": "1"}
        with (mock.patch.dict(os.environ, env, clear=False),
              mock.patch.object(sys, "argv", [str(SCRIPT), "--project-root", str(self.root)]),
              mock.patch.object(module, "load_configuration", return_value=({}, {})),
              mock.patch.object(module, "git_info", return_value={"head": "test", "dirty": False}),
              mock.patch.object(module, "configured_build_toolchain", return_value="stable"),
              mock.patch.object(module.platform, "platform", return_value="test-platform"),
              mock.patch.object(module, "output", return_value="tool version"),
              mock.patch.object(module.shutil, "which", return_value="rustup"),
              mock.patch.object(module.os, "replace", side_effect=replace_side_effect),
              mock.patch.object(module.shutil, "move", side_effect=replace_side_effect),
              mock.patch.object(module.shutil, "rmtree", side_effect=rmtree_side_effect or real_rmtree),
              mock.patch.object(module.subprocess, "run", side_effect=collector_side_effect),
              redirect_stderr(stderr)):
            try:
                return module.main(), stderr.getvalue()
            except OSError as error:
                return error, stderr.getvalue()

    def write_old_outputs(self):
        outputs = {
            "coverage-summary.md": b"old summary\n",
            "coverage-metadata.json": b"old metadata\n",
            "coverage.json": b"old coverage\n",
            "lcov.info": b"old lcov\n",
            "coverage.txt": b"old text\n",
            "coverage-badge.json": b"old badge\n",
            "ci-summary.json": b"old ci summary\n",
            "target/llvm-cov/html/index.html": b"old html\n",
        }
        for relative, content in outputs.items():
            path = self.root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(content)
        return outputs

    def assert_outputs_equal(self, outputs):
        for relative, content in outputs.items():
            path = self.root / relative
            self.assertTrue(path.is_file(), f"{relative} was lost")
            self.assertEqual(path.read_bytes(), content, relative)

    def fake_collection(self, returncode=0):
        def collect(command, *, cwd, **kwargs):
            reports = {
                "coverage-summary.md": "## New coverage\n",
                "coverage.json": '{"data":[{}]}',
                "lcov.info": "TN:\n",
                "coverage.txt": "coverage report\n",
                "coverage-badge.json": "{}",
                "ci-summary.json": "{}",
                "target/llvm-cov/html/index.html": "html",
                "target/llvm-cov/cobertura.xml": "<coverage/>",
            }
            for relative, content in reports.items():
                path = cwd / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(content, encoding="utf-8")
            return subprocess.CompletedProcess(command, returncode)
        return collect

    def backup_directories(self):
        return list(self.root.glob(".coverage-provenance-backup-*"))

    def test_first_backup_move_failure_keeps_old_outputs_and_skips_collection(self):
        outputs = self.write_old_outputs()
        collector = mock.Mock()

        def fail_first_move(source, destination):
            raise OSError("injected first backup move failure")

        result, stderr = self.run_inline(
            replace_side_effect=fail_first_move,
            collector_side_effect=collector,
        )

        collector.assert_not_called()
        self.assert_outputs_equal(outputs)
        self.assertEqual(result, 1, stderr)
        self.assertNotIn("coverage-provenance.log", stderr)
        self.assertEqual(self.backup_directories(), [])

    def test_second_backup_move_failure_restores_all_old_outputs_and_skips_collection(self):
        outputs = self.write_old_outputs()
        move_count = 0
        real_replace = os.replace

        def fail_second_move(source, destination):
            nonlocal move_count
            move_count += 1
            if move_count == 2:
                raise OSError("injected second backup move failure")
            real_replace(source, destination)

        collector = mock.Mock()
        result, stderr = self.run_inline(
            replace_side_effect=fail_second_move,
            collector_side_effect=collector,
        )

        collector.assert_not_called()
        self.assert_outputs_equal(outputs)
        self.assertEqual(result, 1, stderr)
        self.assertNotIn("coverage-provenance.log", stderr)
        self.assertEqual(self.backup_directories(), [])

    def test_collection_failure_restores_old_html_tree_without_old_metadata(self):
        outputs = self.write_old_outputs()
        result, stderr = self.run_inline(
            replace_side_effect=os.replace,
            collector_side_effect=self.fake_collection(returncode=17),
        )

        self.assertEqual(result, 17, stderr)
        self.assertIn("see ", stderr)
        self.assertFalse((self.root / "coverage-metadata.json").exists())
        for relative, content in outputs.items():
            if relative == "coverage-metadata.json":
                continue
            self.assertEqual((self.root / relative).read_bytes(), content, relative)
        self.assertEqual(self.backup_directories(), [])

    def test_failed_restore_keeps_backup_and_continues_restoring_other_outputs(self):
        outputs = self.write_old_outputs()
        real_replace = os.replace

        def fail_coverage_restore(source, destination):
            source = Path(source)
            if source.parent.name.startswith(".coverage-provenance-backup-") and source.name == "coverage.json":
                raise OSError("injected restore failure")
            real_replace(source, destination)

        result, stderr = self.run_inline(
            replace_side_effect=fail_coverage_restore,
            collector_side_effect=self.fake_collection(returncode=17),
        )

        self.assertEqual(result, 17, stderr)
        backups = self.backup_directories()
        self.assertEqual(len(backups), 1)
        self.assertIn(str(backups[0]), stderr)
        self.assertTrue((backups[0] / "coverage.json").is_file())
        self.assertEqual((self.root / "coverage-summary.md").read_bytes(), outputs["coverage-summary.md"])
        self.assertEqual((self.root / "coverage.txt").read_bytes(), outputs["coverage.txt"])

    def test_success_removes_backup_and_preserves_new_reports(self):
        self.write_old_outputs()
        result, stderr = self.run_inline(
            replace_side_effect=os.replace,
            collector_side_effect=self.fake_collection(),
        )

        self.assertEqual(result, 0, stderr)
        self.assertEqual((self.root / "coverage-summary.md").read_text(),
                         "## New coverage\n\n- Provenance: `coverage-metadata.json` (same-run metadata).\n")
        self.assertTrue((self.root / "coverage-metadata.json").is_file())
        self.assertEqual(self.backup_directories(), [])

    def test_backup_cleanup_failure_keeps_new_reports_and_reports_retained_backup(self):
        self.write_old_outputs()
        real_rmtree = shutil.rmtree

        def fail_backup_cleanup(path, *args, **kwargs):
            if Path(path).name.startswith(".coverage-provenance-backup-"):
                raise OSError("injected backup cleanup failure")
            real_rmtree(path, *args, **kwargs)

        result, stderr = self.run_inline(
            replace_side_effect=os.replace,
            collector_side_effect=self.fake_collection(),
            rmtree_side_effect=fail_backup_cleanup,
        )

        self.assertEqual(result, 1, stderr)
        backups = self.backup_directories()
        self.assertEqual(len(backups), 1)
        self.assertIn(str(backups[0]), stderr)
        self.assertTrue((self.root / "coverage-metadata.json").is_file())
        self.assertIn("## New coverage", (self.root / "coverage-summary.md").read_text())

    def test_success_records_local_provenance_and_effective_configuration(self):
        result = self.run_provenance()
        self.assertEqual(result.returncode, 0, result.stderr)
        metadata = json.loads((self.root / "coverage-metadata.json").read_text())
        self.assertEqual(metadata["run"]["kind"], "local")
        self.assertEqual(metadata["coverage"]["features"], ["all-features"])
        self.assertEqual(metadata["coverage"]["scope"], "default-members")
        self.assertEqual(metadata["coverage"]["collector_output"], "coverage.json")
        self.assertEqual(metadata["coverage"]["collection_argv"], [".infra/bin/coverage.sh"])
        self.assertTrue(metadata["config_sha256"][".infra/coverage/coverage.json"])
        self.assertIn("## New coverage\n", (self.root / "coverage-summary.md").read_text())
        hashes = metadata["artifact_sha256"]
        self.assertIn("coverage-summary.md", hashes)
        self.assertIn("coverage-provenance.log", hashes)
        self.assertIn("coverage.json", hashes)
        self.assertIn("target/llvm-cov/html/index.html", hashes)
        self.assertNotIn("coverage-metadata.json", hashes)
        self.assertEqual(metadata["artifacts"], sorted(hashes))
        for relative, expected in hashes.items():
            self.assertEqual(
                hashlib.sha256((self.root / relative).read_bytes()).hexdigest(),
                expected,
                relative,
            )

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
