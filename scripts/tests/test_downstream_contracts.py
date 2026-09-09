"""Deterministic tests for locked downstream checks and source layout."""
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch


def load_script(name):
    """Import a repository script without modifying Python's search path."""
    path = Path(__file__).resolve().parents[1] / f"{name}.py"
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


checker = load_script("check_downstream_contracts")


class LockedChecks(unittest.TestCase):
    """Locked validation must never repair a consumer manifest or lock."""

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        for name in checker.REPOSITORIES:
            folder = self.root / name
            folder.mkdir()
            (folder / "Cargo.toml").write_text("[package]\n")
            (folder / "Cargo.lock").write_text("version = 4\n")

    def test_metadata_only_preserves_locked_arguments(self):
        with patch.object(checker.subprocess, "run") as run:
            checker.run(self.root, True)
        self.assertEqual(run.call_count, 3)
        for call in run.call_args_list:
            self.assertEqual(call.args[0], ["cargo", "metadata", "--locked", "--format-version", "1"])
            self.assertTrue(call.kwargs["check"])

    def test_full_run_checks_every_graph_before_tests(self):
        with patch.object(checker.subprocess, "run") as run:
            checker.run(self.root, False)
        self.assertEqual([call.args[0][1] for call in run.call_args_list], ["metadata"] * 3 + ["test"] * 3)
        for call in run.call_args_list:
            self.assertIn("--locked", call.args[0])

    def test_failed_metadata_does_not_start_tests(self):
        with patch.object(checker.subprocess, "run", side_effect=subprocess.CalledProcessError(101, "cargo")) as run:
            with self.assertRaises(subprocess.CalledProcessError):
                checker.run(self.root, False)
        self.assertEqual(run.call_count, 1)

    def test_missing_required_files_fail_before_cargo(self):
        (self.root / "rs-mime" / "Cargo.lock").unlink()
        with patch.object(checker.subprocess, "run") as run:
            with self.assertRaisesRegex(RuntimeError, "missing required file"):
                checker.run(self.root, False)
        run.assert_not_called()

    def test_lock_drift_is_an_error_even_after_success(self):
        def mutate(*args, **kwargs):
            (self.root / "rs-mime" / "Cargo.lock").write_text("changed")
        with patch.object(checker.subprocess, "run", side_effect=mutate):
            with self.assertRaisesRegex(RuntimeError, "manifest or lock changed"):
                checker.run(self.root, True)


class SourceLayout(unittest.TestCase):
    """Dependency discovery must respect Cargo workspace path bases."""

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.layout = load_script("prepare_downstream_sources")

    def manifest(self, relative, content):
        """Write a small Cargo fixture under the isolated layout root."""
        path = self.root / relative / "Cargo.toml"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)
        return path

    def test_discovers_target_and_workspace_inherited_paths(self):
        self.manifest("rs-a", '[workspace]\nmembers=["member"]\n[workspace.dependencies]\nshared={path="../rs-b"}\n')
        manifest = self.manifest("rs-a/member", '[package]\nname="member"\n[dependencies]\nshared={workspace=true}\n[target.\'cfg(unix)\'.build-dependencies]\nhelper={path="../../rs-c"}\n')
        actual = set(self.layout.path_dependencies(manifest, self.root))
        self.assertEqual(actual, {self.root / "rs-b/Cargo.toml", self.root / "rs-c/Cargo.toml"})

    def test_rejects_paths_outside_layout(self):
        manifest = self.manifest("rs-a", '[dependencies]\noutside={path="../../escape"}\n')
        with self.assertRaisesRegex(RuntimeError, "outside"):
            list(self.layout.path_dependencies(manifest, self.root))

    def test_rejects_symlink_escape(self):
        with tempfile.TemporaryDirectory() as outside:
            (self.root / "rs-outside").symlink_to(outside, target_is_directory=True)
            manifest = self.manifest("rs-a", '[dependencies]\noutside={path="../rs-outside"}\n')
            with self.assertRaisesRegex(RuntimeError, "outside"):
                list(self.layout.path_dependencies(manifest, self.root))

    def test_rejects_missing_inherited_dependency(self):
        self.manifest("rs-a", '[workspace]\nmembers=["member"]\n')
        manifest = self.manifest("rs-a/member", '[dependencies]\nunknown={workspace=true}\n')
        with self.assertRaisesRegex(RuntimeError, "inherited dependency"):
            list(self.layout.path_dependencies(manifest, self.root))

    def test_invalid_repository_name_never_calls_git(self):
        with patch.object(self.layout.subprocess, "run") as run:
            with self.assertRaisesRegex(RuntimeError, "repository name"):
                self.layout.ensure_checkout(self.root, "other-project", "main")
        run.assert_not_called()

    def test_dirty_existing_checkout_is_rejected(self):
        (self.root / "rs-a").mkdir()
        with patch.object(self.layout, "git_output", return_value=" M Cargo.toml"):
            with self.assertRaisesRegex(RuntimeError, "dirty"):
                self.layout.ensure_checkout(self.root, "rs-a", "main")


class CompleteLayout(unittest.TestCase):
    """The coordinated traversal records refs once and handles dependency cycles."""

    setUp = SourceLayout.setUp
    manifest = SourceLayout.manifest

    def test_cycles_and_nested_fixtures_do_not_create_spurious_repositories(self):
        candidate = self.root / "rs-local-files"
        self.manifest("rs-local-files", '[package]\nname="local"\n[dev-dependencies]\nfixture={path="tests/fixture"}\n')
        self.manifest("rs-local-files/tests/fixture", '[dependencies]\nlocal={path="../.."}\n')
        created = []

        def checkout(root, name, ref):
            created.append((name, ref))
            contents = '[package]\nname="example"\n'
            if name == "rs-fs-local":
                contents += '[dependencies]\ncommon={path="../rs-support"}\n'
            elif name == "rs-mime":
                contents += '[dependencies]\nlocal={path="../rs-local-files"}\n'
            elif name == "rs-support":
                contents += '[dependencies]\nconsumer={path="../rs-fs-local"}\n'
            self.manifest(name, contents)
            return root / name

        def output(folder, *arguments):
            return "" if arguments[0] == "status" else "a" * 40

        with patch.object(self.layout, "ensure_checkout", side_effect=checkout), patch.object(self.layout, "git_output", side_effect=output):
            records = self.layout.prepare(self.root, candidate, {"rs-fs-local": "consumer-ref", "rs-mime": "mime-ref", "support": "support-ref"})
        self.assertEqual(created, [("rs-fs-local", "consumer-ref"), ("rs-mime", "mime-ref"), ("rs-support", "support-ref")])
        self.assertEqual(set(records), {"rs-local-files", "rs-fs-local", "rs-mime", "rs-support"})

    def test_explicit_extra_candidate_is_a_traversal_root(self):
        candidate = self.root / "rs-local-files"
        self.manifest("rs-local-files", '[package]\nname="local"\n')
        created = []

        def checkout(root, name, ref):
            created.append((name, ref))
            self.manifest(name, '[package]\nname="fixture"\n')
            return root / name

        with patch.object(self.layout, "ensure_checkout", side_effect=checkout), patch.object(self.layout, "git_output", return_value="a" * 40):
            records = self.layout.prepare(self.root, candidate, {"support": "main", "rs-magika": "candidate-sha"})
        self.assertIn(("rs-magika", "candidate-sha"), created)
        self.assertEqual(records["rs-magika"]["ref"], "candidate-sha")
        self.assertEqual(len([name for name, _ in created if name == "rs-magika"]), 1)

    def test_missing_manifest_after_checkout_is_terminal(self):
        candidate = self.root / "rs-local-files"
        self.manifest("rs-local-files", '[dependencies]\nmissing={path="../rs-extra/nonexistent"}\n')
        def checkout(root, name, ref):
            self.manifest(name, '[package]\nname="fixture"\n')
            return root / name
        with patch.object(self.layout, "ensure_checkout", side_effect=checkout), patch.object(self.layout, "git_output", return_value="a" * 40):
            with self.assertRaisesRegex(RuntimeError, "missing path dependency manifest"):
                self.layout.prepare(self.root, candidate, {"support": "main"})


if __name__ == "__main__":
    unittest.main()
