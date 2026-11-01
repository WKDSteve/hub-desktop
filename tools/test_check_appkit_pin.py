"""Tests of check-appkit-pin.py: every way a pin can be wrong is refused, and only a tag that still
names the pinned commit, for the hub and for the page alike, passes. The remote is never asked:
`remote_commit` and `subprocess.run` are replaced.

    python -m unittest discover tools
"""

import importlib.util
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

SPEC = importlib.util.spec_from_file_location(
    "check_appkit_pin", Path(__file__).with_name("check-appkit-pin.py")
)
pin = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(pin)

SHA = "6111a9b9f78065d411dc502114b244ccf19357b2"
OTHER = "56df6b11ba66e1d9dd918a7284709bfb84e36dcd"
URL = "https://github.com/WKDSteve/appkit"


def lock(*sources):
    """A Cargo.lock with one appkit package per source, and one package from crates.io."""
    names = ["appkit-core", "appkit-hub", "appkit-testkit"]
    blocks = [
        '[[package]]\nname = "serde"\nversion = "1.0.0"\n'
        'source = "registry+https://github.com/rust-lang/crates.io-index"\n'
    ]
    for name, source in zip(names, sources):
        blocks.append(f'[[package]]\nname = "{name}"\nversion = "0.0.0"\nsource = "{source}"\n')
    return "version = 4\n\n" + "\n".join(blocks)


def tagged(tag="v0.1.0", sha=SHA, url=URL):
    return f"git+{url}?tag={tag}#{sha}"


def ui_lock(specifier=None, version=None, web_specifier=None, web_version=None, web=True):
    """A pnpm-lock.yaml as pnpm writes it for the kit (the GitHub short form and the tarball of the commit):
    appkit-ui and, unless `web` is false, appkit-web."""
    specifier = specifier or "github:WKDSteve/appkit#v0.1.0&path:ui-kit"
    version = version or f"https://codeload.github.com/WKDSteve/appkit/tar.gz/{SHA}#path:ui-kit(svelte@5.57.1)"
    web_specifier = web_specifier or "github:WKDSteve/appkit#v0.1.0&path:web-kit"
    web_version = web_version or f"https://codeload.github.com/WKDSteve/appkit/tar.gz/{SHA}#path:web-kit"
    web_block = f"      appkit-web:\n        specifier: {web_specifier}\n        version: {web_version}\n" if web else ""
    return (
        "lockfileVersion: '9.0'\n\nimporters:\n\n  .:\n    dependencies:\n      appkit-ui:\n"
        f"        specifier: {specifier}\n        version: {version}\n"
        + web_block
        + "    devDependencies:\n      vite:\n        specifier: ^8.3.2\n        version: 8.3.4\n"
    )


class Pinned(unittest.TestCase):
    def test_a_tag_and_one_commit_is_a_pin(self):
        packages = list(pin.appkit_packages(lock(tagged(), tagged(), tagged())))
        self.assertEqual(len(packages), 3, "the crates.io package is not an appkit one")
        pins, problems = pin.pinned(packages)
        self.assertEqual(problems, [])
        self.assertEqual(pins, {("v0.1.0", SHA)})

    def test_a_branch_a_rev_or_no_tag_is_refused(self):
        for source in [
            f"git+{URL}?branch=main#{SHA}",
            f"git+{URL}?rev=6111a9b#{SHA}",
            f"git+{URL}?rev=6111a9b&tag=v0.1.0#{SHA}",
            f"git+{URL}#{SHA}",
        ]:
            with self.subTest(source=source):
                _, problems = pin.pinned(pin.appkit_packages(lock(source)))
                self.assertEqual(len(problems), 1, problems)
                self.assertIn("not by a tag", problems[0])

    def test_another_spelling_of_the_repository_is_refused(self):
        for url in [f"{URL}.git", f"{URL}/"]:
            with self.subTest(url=url):
                _, problems = pin.pinned(pin.appkit_packages(lock(tagged(url=url))))
                self.assertEqual(len(problems), 1, problems)
                self.assertIn("not a source of", problems[0])

    def test_packages_on_two_commits_are_refused(self):
        _, problems = pin.pinned(
            pin.appkit_packages(lock(tagged(), tagged(sha=OTHER), tagged()))
        )
        self.assertTrue(any("different commits" in p for p in problems), problems)


class UiPin(unittest.TestCase):
    def test_the_page_pins_the_folder_of_the_kit_by_the_tag(self):
        self.assertEqual(pin.ui_pin(ui_lock()), (("v0.1.0", SHA), []))

    def test_the_git_url_form_of_the_specifier_is_read_too(self):
        text = ui_lock(
            specifier=f"git+{URL}.git#v0.1.0&path:ui-kit",
            version=f"git+{URL}.git#{SHA}&path:ui-kit",
        )
        self.assertEqual(pin.ui_pin(text), (("v0.1.0", SHA), []))

    def test_a_link_to_a_checkout_is_named(self):
        text = ui_lock(specifier="link:../../appkit/ui-kit", version="link:../../appkit/ui-kit")
        result, problems = pin.ui_pin(text)
        self.assertIsNone(result)
        self.assertTrue(any("not the folder ui-kit" in p for p in problems), problems)

    def test_a_page_without_the_kit_is_named(self):
        text = "importers:\n  .:\n    dependencies:\n      vite:\n        specifier: ^8\n        version: 8.3.4\n"
        result, problems = pin.ui_pin(text)
        self.assertIsNone(result)
        self.assertTrue(any("does not pin appkit-ui" in p for p in problems), problems)

    def test_the_web_kit_is_read_from_its_own_folder(self):
        self.assertEqual(pin.kit_pin(ui_lock(), "appkit-web", "web-kit"), (("v0.1.0", SHA), []))
        # The folder of the other package is not this one's.
        text = ui_lock(web_specifier="github:WKDSteve/appkit#v0.1.0&path:ui-kit")
        result, problems = pin.kit_pin(text, "appkit-web", "web-kit")
        self.assertIsNone(result)
        self.assertTrue(any("not the folder web-kit" in p for p in problems), problems)


class Main(unittest.TestCase):
    """main() over both lock files, with the remote replaced; both files are given, because the page's default
    path is relative to the working directory and would be the real lock file of this checkout."""

    def run_main(self, cargo_text, remote, pnpm_text=None, workflows=None):
        with tempfile.TemporaryDirectory() as dir:
            cargo = os.path.join(dir, "Cargo.lock")
            with open(cargo, "w", encoding="utf-8") as f:
                f.write(cargo_text)
            pnpm = os.path.join(dir, "pnpm-lock.yaml")
            with open(pnpm, "w", encoding="utf-8") as f:
                f.write(ui_lock() if pnpm_text is None else pnpm_text)
            flows = os.path.join(dir, "workflows")
            os.mkdir(flows)
            for name, text in (workflows or {}).items():
                with open(os.path.join(flows, name), "w", encoding="utf-8") as f:
                    f.write(text)
            argv = ["check-appkit-pin.py", "--lock", cargo, "--ui-lock", pnpm, "--workflows", flows]
            with mock.patch.object(sys, "argv", argv), mock.patch.object(
                pin, "remote_commit", side_effect=remote
            ), mock.patch("builtins.print") as said:
                code = pin.main()
            words = " ".join(str(call.args[0]) for call in said.call_args_list if call.args)
            return code, words

    def test_a_tag_that_still_names_the_commit_passes_for_the_hub_and_the_page(self):
        code, words = self.run_main(lock(tagged(), tagged(), tagged()), [SHA])
        self.assertEqual(code, 0, words)
        self.assertIn("appkit-ui, appkit-web", words)

    def test_the_release_kit_of_the_workflows_on_the_same_commit_passes(self):
        flow = (
            "jobs:\n  publish:\n    steps:\n"
            f"      - uses: WKDSteve/appkit/release-kit/publish@{SHA} # v0.1.0\n"
            f"      - id: prepare\n        uses: WKDSteve/appkit/release-kit/prepare@{SHA} # v0.1.0\n"
        )
        code, words = self.run_main(lock(tagged(), tagged(), tagged()), [SHA], workflows={"rolling.yml": flow})
        self.assertEqual(code, 0, words)

    def test_the_release_kit_of_a_workflow_on_another_commit_fails(self):
        flow = f"      - uses: WKDSteve/appkit/release-kit/publish@{OTHER} # v0.0.9\n"
        code, words = self.run_main(lock(tagged(), tagged(), tagged()), [SHA], workflows={"release.yml": flow})
        self.assertEqual(code, 1)
        self.assertIn("release.yml", words)
        self.assertIn(OTHER, words)

    def test_a_quoted_other_case_or_root_action_is_read_too(self):
        for line in (
            f'      - uses: "WKDSteve/appkit/release-kit/publish@{OTHER}"\n',
            f"      - uses: WKDSteve/APPKIT/release-kit/publish@{OTHER}\n",
            f"      - uses: WKDSteve/appkit@{OTHER}\n",
        ):
            with self.subTest(line=line):
                code, words = self.run_main(lock(tagged(), tagged(), tagged()), [SHA], workflows={"r.yml": line})
                self.assertEqual(code, 1, words)
                self.assertIn(OTHER, words)

    def test_the_steps_found_are_counted(self):
        flow = f"      - uses: WKDSteve/appkit/release-kit/publish@{SHA}\n"
        code, words = self.run_main(lock(tagged(), tagged(), tagged()), [SHA], workflows={"r.yml": flow})
        self.assertEqual(code, 0, words)
        self.assertIn("1 step(s)", words)

    def test_the_release_kit_by_a_tag_fails(self):
        flow = "        uses: WKDSteve/appkit/release-kit/prepare@v0.1.0\n"
        code, words = self.run_main(lock(tagged(), tagged(), tagged()), [SHA], workflows={"rolling.yaml": flow})
        self.assertEqual(code, 1)
        self.assertIn("not pinned by a full commit", words)

    def test_a_moved_tag_fails(self):
        self.assertEqual(self.run_main(lock(tagged(), tagged(), tagged()), [OTHER])[0], 1)

    def test_a_tag_that_is_gone_fails(self):
        self.assertEqual(self.run_main(lock(tagged(), tagged(), tagged()), [None])[0], 1)

    def test_a_check_that_cannot_ask_is_no_pass(self):
        self.assertEqual(
            self.run_main(lock(tagged(), tagged(), tagged()), RuntimeError("no network"))[0], 2
        )

    def test_a_lock_without_appkit_fails(self):
        self.assertEqual(self.run_main(lock(), [SHA])[0], 1)

    def test_a_page_on_another_commit_than_the_hub_fails(self):
        other = ui_lock(version=f"https://codeload.github.com/WKDSteve/appkit/tar.gz/{OTHER}#path:ui-kit")
        code, words = self.run_main(lock(tagged(), tagged(), tagged()), [SHA], other)
        self.assertEqual(code, 1)
        self.assertIn("one tag for both", words)

    def test_a_page_on_another_tag_than_the_hub_fails(self):
        other = ui_lock(specifier="github:WKDSteve/appkit#v0.2.0&path:ui-kit")
        code, words = self.run_main(lock(tagged(), tagged(), tagged()), [SHA], other)
        self.assertEqual(code, 1)
        self.assertIn("one tag for both", words)

    def test_a_page_with_a_link_fails_before_the_remote_is_asked(self):
        link = ui_lock(specifier="link:../../appkit/ui-kit", version="link:../../appkit/ui-kit")
        code, words = self.run_main(lock(tagged(), tagged(), tagged()), RuntimeError("must not be asked"), link)
        self.assertEqual(code, 1)
        self.assertIn("not the folder ui-kit", words)

    def test_the_web_kit_is_checked_like_the_ui_kit(self):
        """appkit-web: a `link:` to a checkout, a branch, another tag or commit than the hub, or no entry fails."""
        cases = {
            "a link": (
                ui_lock(web_specifier="link:../../appkit/web-kit", web_version="link:../../appkit/web-kit"),
                "appkit-web: not the folder web-kit",
            ),
            "a branch": (ui_lock(web_specifier="github:WKDSteve/appkit#main&path:web-kit"), "appkit-web to ('main'"),
            "another tag": (ui_lock(web_specifier="github:WKDSteve/appkit#v0.2.0&path:web-kit"), "appkit-web to ('v0.2.0'"),
            "another commit": (
                ui_lock(web_version=f"https://codeload.github.com/WKDSteve/appkit/tar.gz/{OTHER}#path:web-kit"),
                f"appkit-web to ('v0.1.0', '{OTHER}')",
            ),
            "no entry": (ui_lock(web=False), "does not pin appkit-web"),
        }
        for label, (text, said) in cases.items():
            with self.subTest(label):
                code, words = self.run_main(lock(tagged(), tagged(), tagged()), RuntimeError("must not be asked"), text)
                self.assertEqual(code, 1, words)
                self.assertIn(said, words)


class RemoteCommit(unittest.TestCase):
    def answer(self, stdout):
        done = subprocess.CompletedProcess(args=[], returncode=0, stdout=stdout, stderr="")
        return mock.patch.object(pin.subprocess, "run", return_value=done)

    def test_the_commit_of_an_annotated_tag_is_the_peeled_one(self):
        lines = f"225ac5e43900b0f95daa5163423c6bbb970fddf7\trefs/tags/v0.1.0\n{SHA}\trefs/tags/v0.1.0^{{}}\n"
        with self.answer(lines):
            self.assertEqual(pin.remote_commit("v0.1.0"), SHA)

    def test_a_lightweight_tag_names_its_commit_itself(self):
        with self.answer(f"{SHA}\trefs/tags/v0.1.0\n"):
            self.assertEqual(pin.remote_commit("v0.1.0"), SHA)


if __name__ == "__main__":
    unittest.main()
