import importlib.util
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "check_freebsd_dependency_pins.py"
spec = importlib.util.spec_from_file_location("freebsd_dependency_pins", SCRIPT)
pins = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pins)


TSV = (
    "../rs-redact\thttps://github.com/qubit-ltd/rs-redact.git\t"
    "04f4bef3ac9a391cc220438918dcb1451a1963a6\n"
    "../rs-json\thttps://github.com/qubit-ltd/rs-json.git\t"
    "0bc4aa9eb0aa2993aa4a75c4162e7ae45f2bebab\n"
)
WORKFLOW = """            git -C ../rs-redact remote add origin https://github.com/qubit-ltd/rs-redact.git
            git -C ../rs-redact fetch --depth 1 origin 04f4bef3ac9a391cc220438918dcb1451a1963a6
            test "$(git -C ../rs-redact rev-parse HEAD)" = 04f4bef3ac9a391cc220438918dcb1451a1963a6
            git -C ../rs-json remote add origin https://github.com/qubit-ltd/rs-json.git
            git -C ../rs-json fetch --depth 1 origin 0bc4aa9eb0aa2993aa4a75c4162e7ae45f2bebab
            test "$(git -C ../rs-json rev-parse HEAD)" = 0bc4aa9eb0aa2993aa4a75c4162e7ae45f2bebab
"""


class FreeBsdDependencyPinsTests(unittest.TestCase):
    def test_current_tsv_and_workflow_pins_match(self):
        root = SCRIPT.parents[1]
        pins.validate(
            (root / ".infra/ci/local-path-dependencies.tsv").read_text(encoding="utf-8"),
            (root / ".github/workflows/ci.yml").read_text(encoding="utf-8"),
        )

    def test_duplicate_tsv_row_fails(self):
        with self.assertRaisesRegex(pins.PinError, "duplicate"):
            pins.validate(TSV + TSV.splitlines()[0] + "\n", WORKFLOW)

    def test_missing_tsv_dependency_fails(self):
        with self.assertRaisesRegex(pins.PinError, "missing"):
            pins.validate(TSV.splitlines()[0] + "\n", WORKFLOW)

    def test_missing_workflow_fetch_or_head_fails(self):
        for line in (
            "            git -C ../rs-json fetch --depth 1 origin 0bc4aa9eb0aa2993aa4a75c4162e7ae45f2bebab\n",
            "            test \"$(git -C ../rs-json rev-parse HEAD)\" = 0bc4aa9eb0aa2993aa4a75c4162e7ae45f2bebab\n",
        ):
            with self.subTest(line=line), self.assertRaisesRegex(pins.PinError, "exactly one"):
                pins.validate(TSV, WORKFLOW.replace(line, ""))

    def test_one_sided_fetch_or_head_change_fails(self):
        changed = "f" * 40
        for line in (
            "            git -C ../rs-json fetch --depth 1 origin 0bc4aa9eb0aa2993aa4a75c4162e7ae45f2bebab",
            "            test \"$(git -C ../rs-json rev-parse HEAD)\" = 0bc4aa9eb0aa2993aa4a75c4162e7ae45f2bebab",
        ):
            with self.subTest(line=line), self.assertRaisesRegex(pins.PinError, "does not match TSV"):
                pins.validate(TSV, WORKFLOW.replace(line, line.rsplit(" ", 1)[0] + " " + changed))


if __name__ == "__main__":
    unittest.main()
