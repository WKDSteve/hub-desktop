"""Tests of tools/sync-app-icons.py: `python -m unittest discover tools`."""

import hashlib
import importlib.util
import json
import pathlib
import tempfile
import unittest

spec = importlib.util.spec_from_file_location(
    "sync_app_icons", pathlib.Path(__file__).with_name("sync-app-icons.py")
)
sync = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sync)

ICON = b'<svg xmlns="http://www.w3.org/2000/svg"></svg>\n'
COMMIT = "a" * 40


def entry(**more):
    return {"repo": "Owner/app", "path": "assets/icons/app/app-small.svg", **more}


class Folder(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.folder = pathlib.Path(temp.name)

    def write(self, icons, files=None):
        (self.folder / "SOURCES.json").write_text(json.dumps({"schema": 1, "icons": icons}), encoding="utf-8")
        for name, data in (files or {}).items():
            (self.folder / name).write_bytes(data)

    def pinned(self):
        self.write({"app": entry(commit=COMMIT, sha256=hashlib.sha256(ICON).hexdigest())}, {"app.svg": ICON})

    def test_what_is_pinned_passes(self):
        self.pinned()
        self.assertEqual(sync.problems(self.folder), [])

    def test_a_file_changed_by_hand_is_found(self):
        self.pinned()
        (self.folder / "app.svg").write_bytes(ICON + b"\n")
        self.assertIn("differs", sync.problems(self.folder)[0])

    def test_a_missing_file_a_stray_file_and_no_pin_are_found(self):
        self.pinned()
        (self.folder / "app.svg").unlink()
        self.assertIn("missing", sync.problems(self.folder)[0])
        self.pinned()
        (self.folder / "other.svg").write_bytes(ICON)
        self.assertIn("other.svg", sync.problems(self.folder)[0])
        (self.folder / "other.svg").unlink()
        self.write({"app": entry()}, {"app.svg": ICON})
        self.assertIn("no commit", sync.problems(self.folder)[0])

    def test_broken_sources_are_a_problem_not_a_crash(self):
        (self.folder / "SOURCES.json").write_text("{", encoding="utf-8")
        self.assertIn("SOURCES.json", sync.problems(self.folder)[0])
        self.write({})
        (self.folder / "SOURCES.json").write_text('{"schema": 2, "icons": {}}', encoding="utf-8")
        self.assertIn("schema", sync.problems(self.folder)[0])

    def test_an_entry_cannot_name_a_path_out_of_the_icons_or_a_file_out_of_the_folder(self):
        for name, value in [
            ("path", "../../secret.svg"),
            ("path", "assets/icons/../../secret.svg"),
            ("path", "assets/icons/app/app.png"),
            ("repo", "no-owner"),
            ("repo", "Owner/app/../other"),
        ]:
            self.assertTrue(sync.entry_problems("app", entry(**{name: value})), value)
        self.assertTrue(sync.entry_problems("../app", entry()))
        self.write({"../app": entry(commit=COMMIT, sha256="x")})
        self.assertTrue(any("plain id" in p for p in sync.problems(self.folder)))

    def test_update_takes_the_file_and_pins_commit_and_sum(self):
        self.write({"app": entry(), "web": entry(path="assets/icons/app/web.svg")})
        asked = []

        def head(repo):
            asked.append(repo)
            return COMMIT

        taken = sync.update(self.folder, head=head, fetch=lambda repo, path, commit: ICON)
        self.assertEqual(taken, ["app", "web"])
        self.assertEqual(asked, ["Owner/app"], "one question per repository")
        self.assertEqual((self.folder / "app.svg").read_bytes(), ICON)
        self.assertEqual(sync.problems(self.folder), [])
        self.assertNotIn(b"\r", (self.folder / "SOURCES.json").read_bytes())

    def test_update_writes_nothing_when_one_icon_is_refused(self):
        self.write({"a": entry(), "b": entry()})
        answers = {"a": ICON, "b": b"<html></html>"}
        files = iter(answers.values())
        with self.assertRaises(ValueError):
            sync.update(self.folder, head=lambda repo: COMMIT, fetch=lambda repo, path, commit: next(files))
        self.assertFalse((self.folder / "a.svg").exists())
        self.assertNotIn("commit", json.loads((self.folder / "SOURCES.json").read_text(encoding="utf-8"))["icons"]["a"])

    def test_update_refuses_an_answer_that_is_no_commit_and_a_bad_entry(self):
        self.write({"a": entry()})
        with self.assertRaises(sync.AskFailed):
            sync.update(self.folder, head=lambda repo: "not found", fetch=lambda repo, path, commit: ICON)
        self.write({"a": entry(path="../x.svg")})
        with self.assertRaises(ValueError):
            sync.update(self.folder, head=lambda repo: COMMIT, fetch=lambda repo, path, commit: ICON)

    def test_what_is_no_icon_is_named(self):
        self.assertIsNone(sync.icon_problem(ICON))
        self.assertEqual(sync.icon_problem(b"<html>"), "no SVG")
        self.assertEqual(sync.icon_problem(b"<svg><SCRIPT>x</SCRIPT></svg>"), "holds a script")
        self.assertEqual(sync.icon_problem(b"<svg>" + b" " * sync.MAX_ICON), "over 64 KiB")


if __name__ == "__main__":
    unittest.main()
