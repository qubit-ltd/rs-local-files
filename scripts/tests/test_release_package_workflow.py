import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/release-package.yml"


class ReleasePackageWorkflowTests(unittest.TestCase):
    def test_release_dispatch_runs_package_validation_outside_ordinary_ci(self):
        self.assertTrue(WORKFLOW.is_file(), "explicit release package workflow is missing")
        workflow = WORKFLOW.read_text(encoding="utf-8")
        self.assertRegex(workflow, r"(?m)^name: Release package validation$")
        self.assertRegex(workflow, r"(?m)^on:\n  workflow_dispatch:\s*$")
        self.assertIn("uses: actions/checkout@v6", workflow)
        self.assertIn("toolchain: 1.94.0", workflow)
        self.assertRegex(
            workflow,
            r"(?m)^        run: \.\/scripts\/check_release_package\.sh$",
        )

        ordinary_ci = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        self.assertNotIn("scripts/check_release_package.sh", ordinary_ci)
        ci_tasks = (ROOT / ".infra/ci/ci.toml").read_text(encoding="utf-8")
        task_list = ci_tasks.splitlines()[0].partition("=")[2]
        self.assertNotIn('"package"', task_list)


if __name__ == "__main__":
    unittest.main()
