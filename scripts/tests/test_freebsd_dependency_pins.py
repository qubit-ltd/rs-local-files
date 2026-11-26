import importlib.util
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "check_freebsd_dependency_pins.py"
spec = importlib.util.spec_from_file_location("freebsd_dependency_pins", SCRIPT)
pins = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pins)


class FreeBsdDependencyPinsTests(unittest.TestCase):
    def test_current_workflow_has_no_stale_sibling_preparation(self):
        root = SCRIPT.parents[1]
        pins.validate(
            (root / ".infra/ci/local-path-dependencies.tsv").read_text(encoding="utf-8"),
            (root / ".github/workflows/ci.yml").read_text(encoding="utf-8"),
        )

    def test_real_path_pin_must_match_freebsd_remote_fetch_and_head(self):
        revision = "a" * 40
        tsv = f"../real-dependency\thttps://example.invalid/real.git\t{revision}\n"
        workflow = f"""            git -C ../real-dependency remote add origin https://example.invalid/real.git
            git -C ../real-dependency fetch --depth 1 origin {revision}
            test "$(git -C ../real-dependency rev-parse HEAD)" = {revision}
"""
        pins.validate(tsv, workflow)
        with self.assertRaisesRegex(pins.PinError, "does not match TSV"):
            pins.validate(tsv, workflow.replace(revision, "b" * 40, 1))

    def test_unused_registry_sibling_pin_is_rejected(self):
        stale = "../rs-redact\thttps://github.com/qubit-ltd/rs-redact.git\t" + "a" * 40 + "\n"
        with self.assertRaisesRegex(pins.PinError, "unused sibling"):
            pins.validate(stale, "")
        with self.assertRaisesRegex(pins.PinError, "workflow still prepares"):
            pins.validate("", "git -C ../rs-json fetch origin HEAD\n")

    def test_duplicate_tsv_row_fails(self):
        row = "../dep\thttps://example.invalid/dep.git\t" + "a" * 40 + "\n"
        with self.assertRaisesRegex(pins.PinError, "duplicate"):
            pins.validate(row + row, "")


if __name__ == "__main__":
    unittest.main()
