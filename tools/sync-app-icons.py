#!/usr/bin/env python3
"""Keep the icons of the other apps as pinned copies of their owners' files (rule APP-11).

An icon belongs to the repository of its app. The hub shows it on a tile, so it needs a copy
at build time: it builds without a network and without access to a private owner, and its
policy forbids loading anything foreign. assets/icons/_borrowed holds `<id>.svg` and
SOURCES.json, which names for every id the owner's repository and path, the commit the copy
was taken from and its SHA-256. Nothing else writes there.

    python tools/sync-app-icons.py               check, offline: what is there is what is pinned
    python tools/sync-app-icons.py --update      take every icon from the main branch of its owner (needs gh)

A new icon is one more entry in SOURCES.json with `repo` and `path`, then --update.

Exit 0: fine. Exit 1: the problems are listed. Exit 2: an owner could not be asked.
"""

import argparse
import hashlib
import json
import pathlib
import re
import subprocess
import sys

BORROWED = pathlib.Path(__file__).resolve().parent.parent / "assets" / "icons" / "_borrowed"
SOURCES = "SOURCES.json"
MAX_ICON = 64 * 1024

PLAIN_ID = re.compile(r"[a-z0-9][a-z0-9-]*\Z")
PLAIN_REPO = re.compile(r"[A-Za-z0-9-]+/[A-Za-z0-9._-]+\Z")
PLAIN_PATH = re.compile(r"assets/icons/[A-Za-z0-9._/-]+\.svg\Z")
FULL_COMMIT = re.compile(r"[0-9a-f]{40}\Z")


class AskFailed(Exception):
    """An owner could not be asked."""


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def icon_problem(data):
    """Why these bytes are no icon to embed, or None. An owner's file is taken, not trusted."""
    if len(data) > MAX_ICON:
        return "over 64 KiB"
    if not data.lstrip().startswith(b"<svg"):
        return "no SVG"
    if b"<script" in data.lower():
        return "holds a script"
    return None


def entry_problems(icon, entry):
    """What is wrong with the `repo` and `path` of one entry of SOURCES.json."""
    found = []
    if not PLAIN_ID.match(icon):
        found.append(f"{icon}: not a plain id")
    repo = str(entry.get("repo"))
    if not PLAIN_REPO.match(repo) or ".." in repo:
        found.append(f"{icon}: repo is not owner/name")
    path = str(entry.get("path"))
    if not PLAIN_PATH.match(path) or ".." in path:
        found.append(f"{icon}: path is no SVG below assets/icons")
    return found


def read_sources(folder):
    try:
        data = json.loads((folder / SOURCES).read_text(encoding="utf-8"))
    except (OSError, ValueError) as e:
        raise ValueError(f"{SOURCES}: {e}") from e
    if not isinstance(data, dict) or data.get("schema") != 1 or not isinstance(data.get("icons"), dict):
        raise ValueError(f"{SOURCES}: not schema 1 with icons")
    if not all(isinstance(entry, dict) for entry in data["icons"].values()):
        raise ValueError(f"{SOURCES}: an entry is no object")
    return data


def problems(folder):
    """Everything in `folder` that is not what SOURCES.json pins. Reads only this folder."""
    try:
        icons = read_sources(folder)["icons"]
    except ValueError as e:
        return [str(e)]
    found = []
    for icon, entry in sorted(icons.items()):
        found += entry_problems(icon, entry)
        if not FULL_COMMIT.match(str(entry.get("commit"))):
            found.append(f"{icon}: no commit pinned (run --update)")
        if not PLAIN_ID.match(icon):
            continue
        try:
            data = (folder / f"{icon}.svg").read_bytes()
        except OSError:
            found.append(f"{icon}: the file is missing")
            continue
        if data and sha256(data) != entry.get("sha256"):
            found.append(f"{icon}: differs from what is pinned (changed by hand? run --update)")
        problem = icon_problem(data)
        if problem:
            found.append(f"{icon}: {problem}")
    known = {SOURCES} | {f"{icon}.svg" for icon in icons}
    found += [f"{p.name}: not named in {SOURCES}" for p in sorted(folder.iterdir()) if p.name not in known]
    return found


def gh(args):
    try:
        done = subprocess.run(["gh", *args], capture_output=True, timeout=60, check=False)
    except (OSError, subprocess.TimeoutExpired) as e:
        raise AskFailed(f"gh: {e}") from e
    if done.returncode != 0:
        raise AskFailed(f"gh {' '.join(args)}: {done.stderr.decode('utf-8', 'replace').strip()}")
    return done.stdout


def head_of(repo):
    return gh(["api", f"repos/{repo}/commits/main", "--jq", ".sha"]).decode("ascii", "replace").strip()


def file_at(repo, path, commit):
    return gh(["api", "-H", "Accept: application/vnd.github.raw+json", f"repos/{repo}/contents/{path}?ref={commit}"])


def update(folder, head=head_of, fetch=file_at):
    """Take every icon from its owner: nothing is written unless all of them are fine."""
    data = read_sources(folder)
    wrong = [p for icon, entry in sorted(data["icons"].items()) for p in entry_problems(icon, entry)]
    if wrong:
        raise ValueError("; ".join(wrong))
    heads = {}
    taken = {}
    for icon, entry in sorted(data["icons"].items()):
        if entry["repo"] not in heads:
            heads[entry["repo"]] = head(entry["repo"])
        commit = heads[entry["repo"]]
        if not FULL_COMMIT.match(commit):
            raise AskFailed(f"{entry['repo']}: no commit in the answer")
        icon_bytes = fetch(entry["repo"], entry["path"], commit)
        problem = icon_problem(icon_bytes)
        if problem:
            raise ValueError(f"{icon}: {entry['repo']}/{entry['path']}: {problem}")
        taken[icon] = (commit, icon_bytes)
    for icon, (commit, icon_bytes) in taken.items():
        (folder / f"{icon}.svg").write_bytes(icon_bytes)
        data["icons"][icon].update(commit=commit, sha256=sha256(icon_bytes))
    with (folder / SOURCES).open("w", encoding="utf-8", newline="\n") as out:
        json.dump(data, out, indent=2, sort_keys=True)
        out.write("\n")
    return sorted(taken)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--update", action="store_true", help="take every icon from its owner")
    args = parser.parse_args()
    if args.update:
        try:
            for icon in update(BORROWED):
                print(f"{icon}: taken")
        except AskFailed as e:
            print(f"error: {e}", file=sys.stderr)
            return 2
        except ValueError as e:
            print(f"error: {e}", file=sys.stderr)
            return 1
    found = problems(BORROWED)
    for line in found:
        print(line)
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main())
