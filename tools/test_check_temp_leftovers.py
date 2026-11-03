"""Tests of check-temp-leftovers.py: what is counted, what --strict and --allow do, and that a temp dir
that cannot be read is no pass. CI runs it on the temp dirs of its jobs (.github/workflows/ci.yml).

    python -m unittest discover tools
"""

import contextlib
import importlib.util
import io
import os
import tempfile
import unittest
from pathlib import Path

SPEC = importlib.util.spec_from_file_location("check_temp_leftovers", Path(__file__).with_name("check-temp-leftovers.py"))
leftover_check = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(leftover_check)


def run(argv):
    out, err = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        try:
            code = leftover_check.main(argv)
        except SystemExit as e:
            code = e.code
    return code, out.getvalue(), err.getvalue()


class Leftovers(unittest.TestCase):
    def setUp(self):
        self._dir = tempfile.TemporaryDirectory(prefix="check-temp-leftovers-test-")
        self.dir = self._dir.name

    def tearDown(self):
        self._dir.cleanup()

    def make(self, *names):
        for name in names:
            os.mkdir(os.path.join(self.dir, name))

    def test_an_empty_dir_passes(self):
        code, out, _ = run(["--dir", self.dir])
        self.assertEqual(code, 0)
        self.assertIn("nothing", out)

    def test_the_folders_of_the_tests_fail_and_others_do_not_count(self):
        self.make("hub-desktop-test-install-abc", "appkit-hub-cli-version-12", "nvim", "chrome_cache")
        code, _, err = run(["--dir", self.dir])
        self.assertEqual(code, 1)
        self.assertIn("2 hub-desktop-*, appkit-* left", err)
        self.assertIn("hub-desktop-test-install-abc", err)
        self.assertNotIn("chrome_cache", err)

    def test_a_file_counts_like_a_folder(self):
        Path(self.dir, "hub-desktop-test-install-42.txt").write_text("x")
        self.assertEqual(run(["--dir", self.dir])[0], 1)

    def test_a_prefix_replaces_the_default_list(self):
        self.make("hub-desktop-x", "other-y")
        self.assertEqual(run(["--dir", self.dir, "--prefix", "other-"])[0], 1)
        self.assertEqual(run(["--dir", self.dir, "--prefix", "none-"])[0], 0)

    def test_strict_counts_every_entry_but_the_allowed_ones(self):
        self.make("nvim", "nvim.0")
        self.assertEqual(run(["--dir", self.dir, "--strict", "--allow", "nvim", "--allow", "nvim.0"])[0], 0)
        self.make("whatever-a-test-called-it")
        code, _, err = run(["--dir", self.dir, "--strict", "--allow", "nvim", "--allow", "nvim.0"])
        self.assertEqual(code, 1)
        self.assertIn("whatever-a-test-called-it", err)

    def test_many_are_listed_up_to_a_limit(self):
        self.make(*[f"hub-desktop-{i:02}" for i in range(35)])
        code, _, err = run(["--dir", self.dir])
        self.assertEqual(code, 1)
        self.assertIn("... and 5 more", err)

    def test_the_prefix_of_ci_finds_the_folders_of_both_install_scripts(self):
        # test-install.ps1 names its folder hub-desktop-test-install-<guid>, test-install.sh (mktemp)
        # hub-desktop-test-install.<random>: the prefix CI passes has neither separator.
        self.make("hub-desktop-test-install-0123abcd", "hub-desktop-test-install.Ab12Cd", "appkit-hub-x")
        code, _, err = run(["--dir", self.dir, "--prefix", "hub-desktop-test-install"])
        self.assertEqual(code, 1)
        self.assertIn("2 hub-desktop-test-install* left", err)

    def test_the_workflows_run_the_check_after_the_install_scripts(self):
        root = Path(__file__).resolve().parent.parent / ".github" / "workflows"
        check = 'python tools/check-temp-leftovers.py --dir "$JOB_TEMP" --prefix hub-desktop-test-install\n'
        for name in ("ci.yml", "release.yml"):
            text = (root / name).read_text(encoding="utf-8")
            self.assertIn("JOB_TEMP=$dir", text, name)
            installs = text.index("sh scripts/test-install.sh")
            self.assertGreater(text.find(check, installs), installs, name)
        ci = (root / "ci.yml").read_text(encoding="utf-8")
        self.assertIn('python tools/check-temp-leftovers.py --dir "$JOB_TEMP" --strict\n', ci)

    def test_a_missing_dir_is_no_pass(self):
        code, _, err = run(["--dir", os.path.join(self.dir, "no-such")])
        self.assertEqual(code, 2)
        self.assertIn("cannot read", err)

    def test_wrong_arguments_are_no_pass(self):
        self.assertEqual(run(["--allow", "nvim"])[0], 2)
        self.assertEqual(run(["--strict", "--prefix", "x"])[0], 2)
        self.assertEqual(run(["--frobnicate"])[0], 2)


if __name__ == "__main__":
    unittest.main()
