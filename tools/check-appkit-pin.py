#!/usr/bin/env python3
"""Fail when the appkit pin of Cargo.lock is not the commit its tag names now.

The hub depends on https://github.com/WKDSteve/appkit by a tag. Cargo.lock pins the commit the
tag named when the lock was written, and a build with --locked takes that commit whatever the tag
says later. A tag that was moved (by mistake, or by someone who got write access) would go
unnoticed. This reads every appkit package of Cargo.lock and asks the repository where its tag
points now.

    python tools/check-appkit-pin.py [--lock Cargo.lock]

Exit 0: every appkit package is pinned by a tag, all by the same tag and commit, the page pins the
kit's folders (`appkit-ui` and `appkit-web` in ui/pnpm-lock.yaml) to that same tag and commit, every action of the
kit a workflow takes (`uses: WKDSteve/appkit/release-kit/...@<sha>`) is pinned by that same
commit, and the tag names that commit. Exit 1: what is wrong is listed. Exit 2: the check itself could not run (git, network); that
is not a pass.

For Cargo.lock what check-lock-age.py is for crates.io: the one git source of the tree.
"""

import argparse
import os
import re
import subprocess
import sys

APPKIT = "https://github.com/WKDSteve/appkit"
SOURCE = re.compile(
    r"^git\+(?P<url>[^?#]+)(?:\?(?P<query>[^#]*))?#(?P<sha>[0-9a-f]{40})$"
)


def appkit_packages(lock_text):
    """Yield (name, source) of every package whose source is the appkit repository."""
    for block in lock_text.split("[[package]]")[1:]:
        name = re.search(r'^name = "([^"]+)"', block, re.M)
        source = re.search(r'^source = "([^"]+)"', block, re.M)
        if name and source and source.group(1).startswith(f"git+{APPKIT}"):
            yield name.group(1), source.group(1)


def pinned(packages):
    """The one (tag, sha) of the packages, or the list of what is wrong."""
    problems = []
    pins = set()
    for name, source in packages:
        match = SOURCE.match(source)
        if not match or match.group("url") != APPKIT:
            problems.append(f"{name}: not a source of {APPKIT}: {source}")
            continue
        query = match.group("query") or ""
        tag = re.fullmatch(r"tag=([^&]+)", query)
        if not tag:
            problems.append(f"{name}: pinned by '{query or 'the default branch'}', not by a tag")
            continue
        pins.add((tag.group(1), match.group("sha")))
    if len(pins) > 1:
        problems.append(f"the appkit packages are pinned to different commits: {sorted(pins)}")
    return pins, problems


# pnpm writes the specifier as `github:WKDSteve/appkit#v0.3.0&path:ui-kit` (also when it was given as a
# git+https URL) and the resolved version as the tarball of the commit, `https://codeload.github.com/
# WKDSteve/appkit/tar.gz/<sha>#path:ui-kit`, with the peers in brackets after it.
def specifier_pattern(folder):
    return re.compile(
        r"^\s*specifier: (?:github:WKDSteve/appkit|git\+https://github\.com/WKDSteve/appkit(?:\.git)?)"
        rf"#(?P<tag>[^&\s]+)&path:{re.escape(folder)}\s*$",
        re.M,
    )


def version_pattern(folder):
    folder = re.escape(folder)
    return re.compile(
        rf"^\s*version: (?:https://codeload\.github\.com/WKDSteve/appkit/tar\.gz/(?P<sha>[0-9a-f]{{40}})#path:{folder}"
        rf"|git\+https://github\.com/WKDSteve/appkit(?:\.git)?#(?P<sha2>[0-9a-f]{{40}})&path:{folder})(?:\([^)]*\))?\s*$",
        re.M,
    )


# The packages the page takes from appkit, each from its folder: all by the tag the hub takes the crates by.
KITS = (("appkit-ui", "ui-kit"), ("appkit-web", "web-kit"))


def kit_pin(lock_text, name, folder):
    """The (tag, sha) the page pins the kit package `name` (the folder `folder` of appkit) to in
    pnpm-lock.yaml, or the list of what is wrong.

    The page takes each package from its folder of the same repository, by the same tag as the hub takes
    the crates; a `link:` to a checkout, a branch or no package at all is a mistake here.
    """
    problems = []
    block = re.search(rf"^\s+{re.escape(name)}:\n(?P<body>(?:\s+\S.*\n){{1,3}})", lock_text, re.M)
    if not block:
        problems.append(f"the page does not pin {name} (ui/pnpm-lock.yaml has no entry)")
        return None, problems
    body = block.group("body")
    spec = specifier_pattern(folder).search(body)
    version = version_pattern(folder).search(body)
    if not spec:
        problems.append(f"{name}: not the folder {folder} of {APPKIT} by a tag: {body.strip().splitlines()[0]}")
        return None, problems
    if not version:
        problems.append(f"{name}: pnpm-lock.yaml pins no commit for it")
        return None, problems
    return (spec.group("tag"), version.group("sha") or version.group("sha2")), problems


def ui_pin(lock_text):
    """kit_pin of `appkit-ui` (the folder ui-kit)."""
    return kit_pin(lock_text, "appkit-ui", "ui-kit")


# A step of a workflow that takes an action of the kit: `uses: WKDSteve/appkit/release-kit/publish@<ref>`,
# also quoted, in another case (GitHub does not tell owner and repository names apart by case) or from the
# root of the repository (`WKDSteve/appkit@<ref>`).
WORKFLOW_USES = re.compile(
    r"""^\s*-?\s*uses:\s*["']?WKDSteve/appkit(?:/(?P<path>[^@\s"']+))?@(?P<ref>[^\s"'#]+)""",
    re.M | re.I,
)


def workflow_pins(texts, sha):
    """What is wrong with the actions of the kit the workflows take: each must be pinned by the full commit
    the crates are pinned to, so that the release-kit of a release is the one of the tag of the hub.

    `texts` maps a file name to its text.
    """
    problems = []
    for name, text in sorted(texts.items()):
        for match in WORKFLOW_USES.finditer(text):
            ref = match.group("ref")
            where = f"{name}: WKDSteve/appkit/{match.group('path') or ''}@{ref}"
            if not re.fullmatch(r"[0-9a-f]{40}", ref):
                problems.append(f"{where}: not pinned by a full commit (NREL-03)")
            elif ref != sha:
                problems.append(f"{where}: Cargo.lock pins appkit at {sha}, one commit for both")
    return problems


def read_workflows(folder):
    """The text of every workflow file in the folder (none when it is not there)."""
    texts = {}
    if os.path.isdir(folder):
        for entry in sorted(os.listdir(folder)):
            if entry.endswith((".yml", ".yaml")):
                with open(os.path.join(folder, entry), encoding="utf-8") as f:
                    texts[entry] = f.read()
    return texts


def remote_commit(tag):
    """The commit the tag names now: the peeled commit of an annotated tag."""
    refs = [f"refs/tags/{tag}", f"refs/tags/{tag}^{{}}"]
    result = subprocess.run(
        ["git", "ls-remote", APPKIT, *refs],
        capture_output=True,
        text=True,
        timeout=60,
        check=False,
    )
    if result.returncode != 0:
        raise RuntimeError(f"git ls-remote failed: {result.stderr.strip()}")
    found = {}
    for line in result.stdout.splitlines():
        sha, ref = line.split("\t", 1)
        found[ref] = sha
    return found.get(refs[1]) or found.get(refs[0])


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--lock", default="Cargo.lock")
    parser.add_argument("--ui-lock", default="ui/pnpm-lock.yaml")
    parser.add_argument("--workflows", default=".github/workflows")
    args = parser.parse_args()
    with open(args.lock, encoding="utf-8") as f:
        packages = list(appkit_packages(f.read()))
    if not packages:
        print(f"no package of {APPKIT} in {args.lock}")
        return 1
    pins, problems = pinned(packages)
    with open(args.ui_lock, encoding="utf-8") as f:
        ui_lock = f.read()
    # The page and the hub take one version of the kit: the same tag, and the commit it named, for every package.
    for name, folder in KITS:
        kit, kit_problems = kit_pin(ui_lock, name, folder)
        problems.extend(kit_problems)
        if kit is not None and pins and kit not in pins:
            problems.append(f"the page pins {name} to {kit}, the hub to {sorted(pins)}: one tag for both")
    # The actions of the release-kit the workflows take: the same commit as the crates.
    workflows = read_workflows(args.workflows)
    steps = sum(len(WORKFLOW_USES.findall(text)) for text in workflows.values())
    if len(pins) == 1:
        problems.extend(workflow_pins(workflows, next(iter(pins))[1]))
    if problems:
        for problem in problems:
            print(problem)
        return 1
    (tag, sha) = pins.pop()
    try:
        now = remote_commit(tag)
    except (OSError, RuntimeError, subprocess.SubprocessError) as err:
        print(f"the check could not run: {err}")
        return 2
    if now is None:
        print(f"the tag {tag} is not in {APPKIT}")
        return 1
    if now != sha:
        print(f"the tag {tag} names {now} now, Cargo.lock pins {sha}: the tag was moved")
        return 1
    names = ", ".join(sorted(name for name, _ in packages))
    # Says how many steps of the workflows it found: none would be a pin that was not looked at.
    print(f"ok: {names}, {', '.join(name for name, _ in KITS)} and {steps} step(s) of the workflows at {tag} = {sha[:12]}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
