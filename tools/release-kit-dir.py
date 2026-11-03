#!/usr/bin/env python3
"""Print the folder release-kit of the appkit that Cargo.lock pins.

The install scripts of a release come from appkit's release-kit (stamped when they are attached), and
scripts/test-install.* try them against this build. They take them from the checkout cargo made of
the pinned commit, so the test and the release use the same file:

    python tools/release-kit-dir.py          # e.g. ~/.cargo/git/checkouts/appkit-.../e032022/release-kit

Exit 0 with the path; exit 1 when cargo has no such checkout (run `cargo fetch --locked` first) or
the pinned appkit has no release-kit.
"""

import json
import pathlib
import subprocess
import sys


def main():
    result = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--locked"],
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        print(f"cargo metadata failed: {result.stderr.strip()}", file=sys.stderr)
        return 1
    for package in json.loads(result.stdout)["packages"]:
        if package["name"] == "appkit-hub" and (package.get("source") or "").startswith("git+"):
            # <checkout>/crates/appkit-hub/Cargo.toml
            kit = pathlib.Path(package["manifest_path"]).parents[2] / "release-kit"
            if not (kit / "scripts" / "install.sh").is_file():
                print(f"the pinned appkit has no release-kit ({kit})", file=sys.stderr)
                return 1
            print(kit.as_posix())
            return 0
    print("no appkit-hub from git in Cargo.lock (a [patch] to a path?)", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())
