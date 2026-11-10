import importlib.util
import tempfile
import unittest
import sys
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[1] / "check-ci-dependency-pins.py"
SPEC = importlib.util.spec_from_file_location("check_ci_dependency_pins", SCRIPT)
CHECKER = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = CHECKER
SPEC.loader.exec_module(CHECKER)

RED = "04f4bef3ac9a391cc220438918dcb1451a1963a6"
JSON = "0bc4aa9eb0aa2993aa4a75c4162e7ae45f2bebab"


def fixture_workflow(red=RED, js=JSON):
    return f"""name: CI
jobs:
  freebsd-ci:
    steps:
      - run: |
          git -C ../rs-redact remote add origin https://github.com/qubit-ltd/rs-redact.git
          git -C ../rs-redact fetch --depth 1 origin {red}
          git -C ../rs-redact checkout --detach FETCH_HEAD
          test \"$(git -C ../rs-redact rev-parse HEAD)\" = {red}
          git -C ../rs-json remote add origin https://github.com/qubit-ltd/rs-json.git
          git -C ../rs-json fetch --depth 1 origin {js}
          git -C ../rs-json checkout --detach FETCH_HEAD
          test \"$(git -C ../rs-json rev-parse HEAD)\" = {js}
  rust-ci:
    uses: shared.yml@main
"""


def checkout_block(repo, sha, name):
    return f"""      - uses: actions/checkout@v6
        with:
          repository: qubit-ltd/{repo}
          ref: {sha}
          path: ci-root/{repo}
"""


def fixture_conditional(red=RED, js=JSON, job_count=3):
    return "jobs:\n" + "\n".join(
        f"  prep-{index}:\n    steps:\n"
        + checkout_block("rs-redact", red, "red")
        + checkout_block("rs-json", js, "json")
        for index in range(job_count)
    )


class DependencyPinsTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        (self.root / ".infra/ci").mkdir(parents=True)
        (self.root / ".github/workflows").mkdir(parents=True)
        (self.root / ".infra/ci/local-path-dependencies.tsv").write_text(
            f"../rs-redact\thttps://github.com/qubit-ltd/rs-redact.git\t{RED}\n"
            f"../rs-json\thttps://github.com/qubit-ltd/rs-json.git\t{JSON}\n",
            encoding="utf-8",
        )
        self.write_workflows()

    def tearDown(self):
        self.temp.cleanup()

    def write_workflows(self, ci=None, conditional=None):
        (self.root / ".github/workflows/ci.yml").write_text(ci or fixture_workflow(), encoding="utf-8")
        (self.root / ".github/workflows/conditional-write.yml").write_text(
            conditional or fixture_conditional(), encoding="utf-8"
        )

    def errors(self):
        errors = []
        deps = CHECKER.read_manifest(self.root, errors)
        CHECKER.check_checkouts(self.root, deps, errors)
        CHECKER.check_freebsd(self.root, deps, errors)
        return errors

    def test_matching_pins_pass(self):
        self.assertEqual(self.errors(), [])

    def test_mismatched_checkout_revision_fails(self):
        self.write_workflows(conditional=fixture_conditional(red="a" * 40))
        self.assertTrue(any("checkout for qubit-ltd/rs-redact" in e for e in self.errors()))

    def test_rs_json_checkout_revision_mismatch_fails(self):
        self.write_workflows(conditional=fixture_conditional(js="a" * 40))
        self.assertTrue(any("checkout for qubit-ltd/rs-json" in e for e in self.errors()))

    def test_missing_ref_fails(self):
        workflow = fixture_conditional().replace(f"          ref: {RED}\n", "")
        self.write_workflows(conditional=workflow)
        self.assertTrue(any("must use manifest revision" in e for e in self.errors()))

    def test_missing_checkout_fails(self):
        workflow = fixture_conditional().replace(checkout_block("rs-json", JSON, "json"), "")
        self.write_workflows(conditional=workflow)
        self.assertTrue(any("lacks checkout(s): qubit-ltd/rs-json" in e for e in self.errors()))

    def test_removed_preparation_job_fails(self):
        self.write_workflows(conditional=fixture_conditional(job_count=2))
        self.assertTrue(any("at least three dependency preparation jobs" in e for e in self.errors()))

    def test_added_preparation_job_is_checked(self):
        extra_job = (
            "  new-preparation-job:\n    steps:\n"
            + checkout_block("rs-redact", RED, "red")
            + checkout_block("rs-json", JSON, "json")
        )
        conditional = fixture_conditional() + extra_job
        self.write_workflows(conditional=conditional)
        self.assertEqual(self.errors(), [])

        mutated_extra = extra_job.replace(f"          ref: {RED}", f"          ref: {'a' * 40}")
        self.write_workflows(conditional=fixture_conditional() + mutated_extra)
        self.assertTrue(any("checkout for qubit-ltd/rs-redact" in e for e in self.errors()))

    def test_changed_freebsd_fetch_fails(self):
        self.write_workflows(ci=fixture_workflow(red="a" * 40))
        self.assertTrue(any("fetch revision differs" in e for e in self.errors()))

    def test_rs_json_freebsd_fetch_revision_mismatch_fails(self):
        workflow = fixture_workflow().replace(
            f"git -C ../rs-json fetch --depth 1 origin {JSON}",
            f"git -C ../rs-json fetch --depth 1 origin {'a' * 40}",
        )
        self.write_workflows(ci=workflow)
        self.assertTrue(any("FreeBSD rs-json fetch revision differs" in e for e in self.errors()))

    def test_changed_freebsd_head_assertion_fails(self):
        workflow = fixture_workflow().replace(f"= {RED}", f"= {'a' * 40}")
        self.write_workflows(ci=workflow)
        self.assertTrue(any("HEAD assertion differs" in e for e in self.errors()))

    def test_rs_json_freebsd_head_assertion_mismatch_fails(self):
        workflow = fixture_workflow().replace(
            f"rev-parse HEAD)\" = {JSON}", f"rev-parse HEAD)\" = {'a' * 40}"
        )
        self.write_workflows(ci=workflow)
        self.assertTrue(any("FreeBSD rs-json HEAD assertion differs" in e for e in self.errors()))

    def test_changed_freebsd_url_fails(self):
        self.write_workflows(ci=fixture_workflow().replace("rs-redact.git", "other.git"))
        self.assertTrue(any("remote URL differs" in e for e in self.errors()))

    def test_rs_json_freebsd_url_mismatch_fails(self):
        workflow = fixture_workflow().replace(
            "https://github.com/qubit-ltd/rs-json.git",
            "https://github.com/qubit-ltd/other.git",
        )
        self.write_workflows(ci=workflow)
        self.assertTrue(any("FreeBSD rs-json remote URL differs" in e for e in self.errors()))

    def test_duplicate_manifest_path_fails(self):
        path = self.root / ".infra/ci/local-path-dependencies.tsv"
        path.write_text(path.read_text() + path.read_text().splitlines()[1] + "\n", encoding="utf-8")
        self.assertTrue(any("duplicate dependency" in e for e in self.errors()))

    def test_invalid_manifest_sha_fails(self):
        path = self.root / ".infra/ci/local-path-dependencies.tsv"
        path.write_text(path.read_text().replace(RED, "deadbeef"), encoding="utf-8")
        self.assertTrue(any("full 40-character" in e for e in self.errors()))

    def test_manifest_path_must_map_to_expected_repository(self):
        path = self.root / ".infra/ci/local-path-dependencies.tsv"
        path.write_text(path.read_text().replace("rs-redact.git", "rs-json.git", 1), encoding="utf-8")
        self.assertTrue(any("must map to" in e for e in self.errors()))

    def test_dynamic_ref_fails(self):
        workflow = fixture_conditional().replace(f"          ref: {RED}", "          ref: ${{ github.sha }}")
        self.write_workflows(conditional=workflow)
        self.assertTrue(any("must use manifest revision" in e for e in self.errors()))

    def test_missing_preparation_job_fails(self):
        self.write_workflows(ci="jobs:\n  build:\n    steps: []\n")
        self.assertTrue(any("FreeBSD dependency preparation" in e for e in self.errors()))


if __name__ == "__main__":
    unittest.main()
