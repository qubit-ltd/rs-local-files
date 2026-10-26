"""Deterministic tests for locked downstream checks and source layout."""
import importlib.util
import json
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


def metadata_fixture(local_path="/candidate/Cargo.toml", io_path="/io/Cargo.toml", include_local=True):
    packages = [
        {"id": "consumer 1.0.0 (path+file:///consumer)", "name": "consumer", "manifest_path": "/consumer/Cargo.toml"},
        {"id": "qubit-io 0.19.0 (path+file:///io)", "name": "qubit-io", "manifest_path": io_path},
    ]
    deps = [{"pkg": packages[1]["id"], "name": "qubit_io", "dep_kinds": []}]
    if include_local:
        packages.append({"id": "qubit-local-files 0.5.0 (path+file:///candidate)", "name": "qubit-local-files", "manifest_path": local_path})
        deps.append({"pkg": packages[2]["id"], "name": "qubit_local_files", "dep_kinds": []})
    return {"packages": packages,
            "resolve": {"root": packages[0]["id"], "nodes": [
                {"id": packages[0]["id"], "deps": deps},
                *[{"id": package["id"], "deps": []} for package in packages[1:]],
            ]}}


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

    def test_graph_accepts_reachable_candidate_and_same_source_io(self):
        checker.validate_candidate_metadata(metadata_fixture(), "consumer 1.0.0 (path+file:///consumer)", {
            "qubit-local-files": Path("/candidate/Cargo.toml"), "qubit-io": Path("/io/Cargo.toml")})

    def test_candidate_present_but_unreachable_is_rejected(self):
        metadata = metadata_fixture(include_local=False)
        metadata["packages"].append({"id": "qubit-local-files 0.5.0 (registry)", "name": "qubit-local-files", "manifest_path": "/registry/Cargo.toml"})
        with self.assertRaisesRegex(RuntimeError, "reachable qubit-local-files package missing"):
            checker.validate_candidate_metadata(metadata, "consumer 1.0.0 (path+file:///consumer)", {
                "qubit-local-files": Path("/candidate/Cargo.toml"), "qubit-io": Path("/io/Cargo.toml")})

    def test_registry_only_source_is_rejected(self):
        metadata = metadata_fixture(local_path="/registry/Cargo.toml")
        with self.assertRaisesRegex(RuntimeError, "source identity mismatch"):
            checker.validate_candidate_metadata(metadata, "consumer 1.0.0 (path+file:///consumer)", {
                "qubit-local-files": Path("/candidate/Cargo.toml"), "qubit-io": Path("/io/Cargo.toml")})

    def test_dual_source_for_same_package_name_is_rejected(self):
        metadata = metadata_fixture()
        registry = {"id": "qubit-local-files 0.5.0 (registry)", "name": "qubit-local-files", "manifest_path": "/registry/Cargo.toml"}
        metadata["packages"].append(registry)
        metadata["resolve"]["nodes"].append({"id": registry["id"], "deps": []})
        metadata["resolve"]["nodes"][0]["deps"].append({"pkg": registry["id"], "name": "qubit_local_files", "dep_kinds": []})
        with self.assertRaisesRegex(RuntimeError, "source identity mismatch"):
            checker.validate_candidate_metadata(metadata, "consumer 1.0.0 (path+file:///consumer)", {
                "qubit-local-files": Path("/candidate/Cargo.toml"), "qubit-io": Path("/io/Cargo.toml")})

    def test_wrong_io_source_is_rejected(self):
        with self.assertRaisesRegex(RuntimeError, "qubit-io source identity mismatch"):
            checker.validate_candidate_metadata(metadata_fixture(io_path="/registry/qubit-io/Cargo.toml"),
                                                "consumer 1.0.0 (path+file:///consumer)", {
                "qubit-local-files": Path("/candidate/Cargo.toml"), "qubit-io": Path("/io/Cargo.toml")})

    def test_missing_root_and_resolve_are_rejected(self):
        metadata = metadata_fixture()
        with self.assertRaisesRegex(RuntimeError, "root package metadata is missing"):
            checker.validate_candidate_metadata(metadata, "missing", {})
        with self.assertRaisesRegex(RuntimeError, "no resolve graph"):
            checker.validate_candidate_metadata({"packages": metadata["packages"]}, "consumer 1.0.0 (path+file:///consumer)", {})


class SourceLayout(unittest.TestCase):
    """Dependency discovery must respect Cargo workspace path bases."""

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        # Match prepare() before calling helpers, including Windows 8.3 aliases.
        self.root = Path(self.directory.name).resolve()
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

    def test_empty_workspace_member_pattern_is_ignored(self):
        manifest = self.manifest("rs-a", '[workspace]\nmembers=[""]\n')
        with patch.object(Path, "glob") as glob:
            self.assertEqual(list(self.layout.workspace_member_manifests(manifest, self.root)), [])
        glob.assert_not_called()

    def test_unpublished_mime_dependencies_use_latest_sibling_paths(self):
        self.manifest("rs-command", '[package]\nname="qubit-command"\nversion="0.11.0"\n')
        self.manifest("rs-local-files", '[package]\nname="qubit-local-files"\nversion="0.5.0"\n')
        self.manifest("rs-config", '[package]\nname="qubit-config"\nversion="0.15.0"\n')
        lockfile = self.root / "rs-mime/Cargo.lock"
        lockfile.parent.mkdir(parents=True)
        lockfile.write_text('version = 4\n'
                            '[[package]]\nname="qubit-command"\nversion="0.11.0"\n'
                            '[[package]]\nname="qubit-local-files"\nversion="0.5.0"\n'
                            '[[package]]\nname="qubit-config"\nversion="0.15.0"\n')
        mime = self.manifest("rs-mime", '[dependencies]\nqubit-command="0.11"\n'
                             'qubit-local-files="0.5"\n'
                             'qubit-config={version="0.15", default-features=false}\n')
        checked_out = []

        def checkout(name):
            checked_out.append(name)
            return self.root / name

        self.layout.use_latest_local_dependencies(mime, self.root, checkout)

        dependencies = self.layout.manifest_data(mime)["dependencies"]
        self.assertEqual(dependencies["qubit-command"], {
            "path": "../rs-command", "version": "0.11"})
        self.assertEqual(dependencies["qubit-local-files"], {
            "path": "../rs-local-files", "version": "0.5"})
        self.assertEqual(dependencies["qubit-config"], {
            "path": "../rs-config", "version": "0.15", "default-features": False})
        self.assertEqual(checked_out, ["rs-command", "rs-local-files", "rs-config"])
        self.assertEqual(set(self.layout.path_dependencies(mime, self.root)), {
            self.root / "rs-command/Cargo.toml", self.root / "rs-local-files/Cargo.toml",
            self.root / "rs-config/Cargo.toml"})

    def test_missing_existing_path_dependency_checks_out_latest_sibling(self):
        lockfile = self.root / "rs-consumer/Cargo.lock"
        lockfile.parent.mkdir(parents=True)
        lockfile.write_text('version = 4\n[[package]]\nname="qubit-redact"\nversion="0.10.0"\n')
        manifest = self.manifest(
            "rs-consumer",
            '[dependencies]\nqubit-redact={path="../rs-redact", version="0.10", default-features=false}\n',
        )
        (self.root / "rs-redact").mkdir()
        (self.root / "rs-redact/Cargo.toml").write_text(
            '[package]\nname="qubit-redact"\nversion="0.10.0"\n'
        )
        checkouts = []

        def checkout(name):
            checkouts.append(name)
            return self.root / name

        self.layout.use_latest_local_dependencies(manifest, self.root, checkout)

        self.assertEqual(checkouts, ["rs-redact"])
        self.assertEqual(
            self.layout.manifest_data(manifest)["dependencies"]["qubit-redact"],
            {"path": "../rs-redact", "version": "0.10", "default-features": False},
        )

    def test_same_local_dependency_in_multiple_tables_gets_sibling_path(self):
        self.manifest("rs-redact", '[package]\nname="qubit-redact"\nversion="0.10.0"\n')
        manifest = self.manifest(
            "rs-consumer",
            '[package]\nname="consumer"\n'
            '[dependencies]\nqubit-redact="0.10"\n'
            '[dev-dependencies]\nqubit-redact="0.10"\n',
        )
        lockfile = manifest.with_name("Cargo.lock")
        lockfile.write_text('version = 4\n[[package]]\nname="qubit-redact"\nversion="0.10.0"\n')

        self.layout.use_latest_local_dependencies(manifest, self.root, lambda name: self.root / name)

        data = self.layout.manifest_data(manifest)
        self.assertEqual(data["dependencies"]["qubit-redact"], {
            "path": "../rs-redact", "version": "0.10"})
        self.assertEqual(data["dev-dependencies"]["qubit-redact"], {
            "path": "../rs-redact", "version": "0.10"})

    def test_candidate_registry_source_wins_over_stale_path_lock_source(self):
        manifest = self.manifest(
            "rs-a",
            '[dependencies]\nqubit-budget={path="../rs-budget", version="0.9.1", features=["json"]}\n',
        )
        lockfile = self.root / "rs-a/Cargo.lock"
        lockfile.write_text('version = 4\n[[package]]\nname="qubit-budget"\nversion="0.9.1"\n')
        checkout_calls = []

        def checkout(name):
            checkout_calls.append(name)
            return self.root / name

        self.layout.use_latest_local_dependencies(manifest, self.root, checkout, {"qubit-budget"})

        self.assertEqual(
            self.layout.manifest_data(manifest)["dependencies"]["qubit-budget"],
            {"version": "0.9.1", "features": ["json"]},
        )
        self.assertEqual(checkout_calls, [])

    def test_workspace_inherited_unpublished_dependency_uses_sibling_path(self):
        self.manifest("rs-redact", '[package]\nname="qubit-redact"\nversion="0.10.0"\n')
        workspace = self.manifest(
            "rs-consumer",
            '[workspace]\nmembers=["member"]\n'
            '[workspace.dependencies]\nqubit-redact="0.10"\n',
        )
        member = self.manifest(
            "rs-consumer/member",
            '[package]\nname="consumer"\nversion="1.0.0"\n'
            '[dependencies]\nqubit-redact={workspace=true}\n',
        )
        (self.root / "rs-consumer/Cargo.lock").write_text(
            'version = 4\n[[package]]\nname="qubit-redact"\nversion="0.10.0"\n'
        )

        self.layout.use_latest_local_dependencies(workspace, self.root, lambda name: self.root / "rs-redact")

        self.assertEqual(
            self.layout.manifest_data(workspace)["workspace"]["dependencies"]["qubit-redact"],
            {"path": "../rs-redact", "version": "0.10"},
        )
        self.assertTrue(self.layout.manifest_data(member)["dependencies"]["qubit-redact"]["workspace"])

    def test_path_only_local_dependency_is_valid(self):
        self.manifest("rs-redact", '[package]\nname="qubit-redact"\nversion="0.10.0"\n')
        manifest = self.manifest(
            "rs-consumer",
            '[dependencies]\nqubit-redact={path="../rs-redact"}\n',
        )
        manifest.with_name("Cargo.lock").write_text(
            'version = 4\n[[package]]\nname="qubit-redact"\nversion="0.10.0"\n'
        )

        self.layout.use_latest_local_dependencies(manifest, self.root, lambda name: self.root / "rs-redact")

        self.assertEqual(
            self.layout.manifest_data(manifest)["dependencies"]["qubit-redact"],
            {"path": "../rs-redact"},
        )

    def test_fixture_qubit_io_paths_all_use_one_copy_without_touching_source(self):
        fixture = self.root / "candidate-fixture"
        original_io = self.manifest("rs-io", '[package]\nname="qubit-io"\n')
        self.manifest("rs-local-files", '[package]\nname="candidate"\n')
        consumer = self.manifest("rs-fs-local", '[package]\nname="consumer"\n'
                                 '[dependencies]\nqubit-io={path="../rs-io"}\n'
                                 '[dev-dependencies]\nqubit-io={path="../rs-io"}\n'
                                 '[target.\'cfg(unix)\'.dependencies]\nqubit-io={path="../rs-io"}\n')
        workspace = self.manifest("rs-mime", '[workspace]\nmembers=["member"]\n'
                                  '[workspace.dependencies]\nqubit-io="0.19"\n')
        member = self.manifest("rs-mime/member", '[package]\nname="member"\n'
                               '[dependencies]\nqubit-io={workspace=true}\n')
        fixture.mkdir()
        for name in ("rs-io", "rs-fs-local", "rs-mime", "rs-local-files"):
            source = self.root / name
            import shutil
            shutil.copytree(source, fixture / name)
        invalid_fixture = fixture / "rs-fs-local/tests/fixtures/crates/serde_disabled/Cargo.toml"
        invalid_fixture.parent.mkdir(parents=True)
        invalid_fixture.write_text("this is intentionally not valid TOML = {", encoding="utf-8")

        self.layout.redirect_fixture_io_paths(fixture)
        self.assertTrue(invalid_fixture.exists())

        manifest = fixture / "rs-fs-local/Cargo.toml"
        self.assertEqual({path for path in self.layout.path_dependencies(manifest, fixture)
                          if path.parent.name == "rs-io"}, {fixture / "rs-io/Cargo.toml"})
        copied_member = fixture / "rs-mime/member/Cargo.toml"
        self.assertEqual(list(self.layout.path_dependencies(copied_member, fixture)), [fixture / "rs-io/Cargo.toml"])
        self.assertIn('path="../rs-io"', consumer.read_text())
        self.assertIn('path = "../rs-io"', (fixture / "rs-fs-local/Cargo.toml").read_text())
        self.assertIn('qubit-io="0.19"', workspace.read_text())
        self.assertIn('path = "../rs-io"', (fixture / "rs-mime/Cargo.toml").read_text())
        self.assertEqual(original_io.read_text(), '[package]\nname="qubit-io"\n')

    def test_cargo_config_patches_resolve_to_fixture_sources(self):
        fixture = self.root / "candidate-fixture"
        for relative, contents in {
            "rs-io/Cargo.toml": '[package]\nname="qubit-io"\nversion="0.19.0"\nedition="2021"\n',
            "rs-local-files/Cargo.toml": '[package]\nname="qubit-local-files"\nversion="0.5.0"\nedition="2021"\n[dependencies]\nqubit-io="0.19"\n',
            "consumer/Cargo.toml": '[package]\nname="consumer"\nversion="0.1.0"\nedition="2021"\n[dependencies]\nqubit-local-files="0.5"\nqubit-io="0.19"\n',
        }.items():
            path = fixture / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(contents, encoding="utf-8")
            (path.parent / "src").mkdir()
            (path.parent / "src/lib.rs").write_text("", encoding="utf-8")

        self.layout.write_candidate_config(fixture)
        result = subprocess.run(["cargo", "metadata", "--offline", "--format-version", "1"],
                                cwd=fixture / "consumer", check=True, text=True,
                                stdout=subprocess.PIPE)
        packages = json.loads(result.stdout)["packages"]
        manifests = {package["name"]: Path(package["manifest_path"]).resolve() for package in packages}
        self.assertEqual(manifests["qubit-local-files"], (fixture / "rs-local-files/Cargo.toml").resolve())
        self.assertEqual(manifests["qubit-io"], (fixture / "rs-io/Cargo.toml").resolve())

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

    def test_prepare_rewrites_only_fixture_and_processes_workspace_members(self):
        candidate = self.root / "rs-local-files"
        self.manifest("rs-local-files", '[package]\nname="local"\nversion="0.5.0"\n')
        original_workspace = (
            '[workspace]\nmembers=["member"]\n'
            '[workspace.dependencies]\nqubit-redact="0.10"\n'
        )
        original_member = (
            '[package]\nname="mime-member"\nversion="1.0.0"\n'
            '[dependencies]\nqubit-redact={workspace=true}\nqubit-command="0.11"\n'
        )
        original_lock = ('version = 4\n[[package]]\nname="qubit-redact"\nversion="0.10.0"\n'
                         '[[package]]\nname="qubit-command"\nversion="0.11.0"\n')
        self.manifest("rs-mime", original_workspace)
        self.manifest("rs-mime/member", original_member)
        (self.root / "rs-mime/Cargo.lock").write_text(original_lock)
        created = []

        def checkout(root, name, ref):
            created.append(name)
            if name == "rs-mime":
                self.manifest(name, original_workspace)
                self.manifest("rs-mime/member", original_member)
                (root / name / "Cargo.lock").write_text(original_lock)
            else:
                package_name = name.replace("rs-", "qubit-")
                version = "0.10.0" if name == "rs-redact" else "0.11.0"
                self.manifest(name, f'[package]\nname="{package_name}"\nversion="{version}"\n')
            return root / name

        with patch.object(self.layout, "ensure_checkout", side_effect=checkout), \
                patch.object(self.layout, "git_output", side_effect=lambda folder, *args: "" if args[0] == "status" else "a" * 40), \
                patch.object(self.layout.subprocess, "run"):
            records = self.layout.prepare(
                self.root,
                candidate,
                {"rs-fs-local": "fs-ref", "rs-mime": "mime-ref", "support": "support-ref"},
            )

        fixture_workspace = self.root / "candidate-fixture/rs-mime/Cargo.toml"
        fixture_member = self.root / "candidate-fixture/rs-mime/member/Cargo.toml"
        self.assertEqual(self.layout.manifest_data(fixture_workspace)["workspace"]["dependencies"]["qubit-redact"], {
            "path": "../rs-redact", "version": "0.10"})
        self.assertTrue(self.layout.manifest_data(fixture_member)["dependencies"]["qubit-redact"]["workspace"])
        self.assertEqual(self.layout.manifest_data(fixture_member)["dependencies"]["qubit-command"], {
            "path": "../../rs-command", "version": "0.11"})
        self.assertEqual((self.root / "rs-mime/Cargo.toml").read_text(), original_workspace)
        self.assertEqual((self.root / "rs-mime/member/Cargo.toml").read_text(), original_member)
        self.assertEqual((self.root / "rs-mime/Cargo.lock").read_text(), original_lock)
        self.assertIn("rs-redact", created)
        self.assertIn("rs-command", created)
        self.assertIn("rs-redact", records)
        self.assertIn("rs-command", records)

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
                contents += ('[dependencies]\nqubit-command="0.11"\n'
                             'qubit-local-files="0.5"\nlocal={path="../rs-local-files"}\n')
            elif name == "rs-support":
                contents += '[dependencies]\nconsumer={path="../rs-fs-local"}\n'
            self.manifest(name, contents)
            return root / name

        def output(folder, *arguments):
            return "" if arguments[0] == "status" else "a" * 40

        with patch.object(self.layout, "ensure_checkout", side_effect=checkout), patch.object(self.layout, "git_output", side_effect=output), patch.object(self.layout.subprocess, "run"):
            records = self.layout.prepare(self.root, candidate, {"rs-fs-local": "consumer-ref", "rs-mime": "mime-ref", "support": "support-ref"})
        self.assertEqual(created, [("rs-fs-local", "consumer-ref"), ("rs-mime", "mime-ref"), ("rs-io", "support-ref"), ("rs-support", "support-ref")])
        self.assertEqual(set(records), {"rs-local-files", "rs-fs-local", "rs-mime", "rs-io", "rs-support", "candidate-fixture"})

    def test_explicit_extra_candidate_is_a_traversal_root(self):
        candidate = self.root / "rs-local-files"
        self.manifest("rs-local-files", '[package]\nname="local"\n')
        created = []

        def checkout(root, name, ref):
            created.append((name, ref))
            self.manifest(name, '[package]\nname="fixture"\n')
            return root / name

        with patch.object(self.layout, "ensure_checkout", side_effect=checkout), patch.object(self.layout, "git_output", return_value="a" * 40), patch.object(self.layout.subprocess, "run"):
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
        with patch.object(self.layout, "ensure_checkout", side_effect=checkout), patch.object(self.layout, "git_output", return_value="a" * 40), patch.object(self.layout.subprocess, "run"):
            with self.assertRaisesRegex(RuntimeError, "missing path dependency manifest"):
                self.layout.prepare(self.root, candidate, {"support": "main"})


if __name__ == "__main__":
    unittest.main()
