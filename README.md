# hub-desktop

One place to start every `*-desktop` and `*-web` app: a local hub that shows each app as a tile and
opens it as a native app or as a web app with a single click.

> **Status: first milestone.** The hub lists the installed apps as tiles and starts them with one click, natively or
> as a web app. There is no versioned release yet; the newest state of `main` is on the
> [`latest` pre-release](https://github.com/WKDSteve/hub-desktop/releases/tag/latest).

## Install (Windows)

In your own PowerShell (not elevated; no administrator rights, no SmartScreen stop):

```
powershell -ExecutionPolicy Bypass -c "irm https://github.com/WKDSteve/hub-desktop/releases/download/latest/install.ps1 | iex"
```

Add `-InstallArgs '--shortcut'` (run the script as a script block) for a link on the desktop. `hub-desktop uninstall`
and `uninstall.ps1` take it away again. The hub needs none of the other apps to run.

## What it does

- **Installed apps** appear as tiles: `tasks-desktop`, `rules-desktop`, `docmap-desktop`, `ai-desktop`
  and the apps that follow. One click starts the app (its own hub first, if it is not running) and
  opens it, natively or as its web variant. Nothing else needs to be clicked.
- **Apps that are not installed** appear darkened in a separate area, *Install or download*, with a
  link to the release page and its checksums.
- Later: the state of each app (running, update available), watching projects with testing.nvim,
  handing work from one app to another, and report viewers.

## How it is built

`hub-desktop` follows its sibling apps:

- a small web server on your own machine (`127.0.0.1` only), built on
  [appkit](https://github.com/WKDSteve/appkit), with its guard and pairing;
- the page in Svelte 5 on appkit's `ui-kit`;
- a catalog of the known apps compiled into the program: it only names fixed ways to detect and
  start an app, never a command line;
- a web variant `hub-web` (one HTML file, no server) for machines where no native program may run.

## Build and run from source

```
cd ui && pnpm install --frozen-lockfile && pnpm build && cd ..
cargo build --release --locked
target/release/hub-desktop            # runs the hub in this window and opens the browser
```

`hub-desktop open --launch` opens a paired page for a hub that runs, `hub-desktop stop` ends it. Checks before a
change: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`,
`cargo test --workspace --locked` and, in `ui/`, `pnpm check && pnpm test`.

## License

[MIT](LICENSE)

## Web hub (for machines that may not run programs)

`hub.html` is the same page as one file that is opened from disk: no program, no server. It lists the web variants
(`tasks-web`, ...) as links to the page next to it, so clone the `*-web` repositories side by side in one folder. A
program is shown as "native only". The page cannot look at your machine: an app that is not there is one you mark
yourself ("Mark as not present" in the menu of the tile), or all at once: **Choose the folder of the clones** (Chrome
and Edge) opens the folder dialog, and every web variant whose page is not in the folder you pick is marked, the rest
unmarked. The page reads only the names in that folder and keeps nothing of it but the marks. Build it with `cd ui && pnpm web:build`
(`ui/dist-web/hub.html`).

## Hide, order and stop

Put a file `catalog-overlay.json` into the config folder of the hub to hide or order tiles:

```json
{ "schema": "hub-catalog/1", "hidden": ["streamtap"], "order": ["tasks", "rules"], "web_root": "C:/repos" }
```

It can only hide, order and name the folder of the web variants (`config.json` wins when it names one too). A
newer schema (`hub-catalog/2`) is refused, keys it does not know are ignored, and a program or a way to start one
cannot be put in it. A file that cannot be used is reported in the console of the hub and ignored. A hidden app is
not listed, not started and its page is not served.

An app built on the kit (Tasks, Rules) that runs has a **Stop** entry in the menu of its tile: the hub runs
`<program> stop` of that app, nothing else.

## Testing monitor

Name the export folder of [testing.nvim](https://github.com/WKDSteve/testing.nvim) in `config.json`
(`{ "testing_export": "C:/results/export" }`, an absolute folder) and the page shows a **Testing** section with a
light for each repository: red, check (partly run, outdated, CI running or unknown), green, or no result. The worst
of the local result, the local pipeline and the CI wins. The hub reads the folder (`testing export --out`) and starts
nothing; the history and the start of a run stay with testing.nvim. A folder that cannot be read is said in one
sentence, without its path.
