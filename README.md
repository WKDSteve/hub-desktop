# hub-desktop

One place to start every `*-desktop` and `*-web` app: a local hub that shows each app as a tile and
opens it as a native app or as a web app with a single click.

> **Status: scaffold.** The hub starts, pairs with the browser and shows an empty page. The list of apps, the
> start of an app and a release are the next steps of the first milestone, described below. There is no release yet.

## What it will do

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
