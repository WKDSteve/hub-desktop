# hub-web

The page of [hub-desktop](https://github.com/WKDSteve/hub-desktop) as a single HTML file that Chrome or Edge opens
from disk (`file://`). No install, no server, no host: the file makes no network request (its policy forbids it). It
is meant for machines where no program of your own may run.

This repository holds the built page `hub.html` and its sum (the sources live in hub-desktop; its workflow `web.yml`
writes everything here, do not edit it by hand), plus this README and the license files.

## Use

Clone the web variants side by side in one folder, `hub-web` among them:

```
cd C:\repos
git clone https://github.com/WKDSteve/hub-web.git
git clone https://github.com/WKDSteve/tasks-web.git
```

1. Check the file: `Get-FileHash hub-web\hub.html` must match the line in `hub-web\SHA256SUMS`.
2. Open `file:///C:/repos/hub-web/hub.html` in Chrome or Edge (adjust the path).
3. Each web variant is a tile; a click opens its page next to `hub-web` (`../tasks-web/tasks.html`). A program of
   hub-desktop that has no web variant shows as "Native only".
4. **Choose the folder of the clones** (Chrome and Edge) and pick the folder that holds the clones: every web
   variant whose page is not there is marked as not present, the rest unmarked. The page reads only the names in that
   folder and keeps nothing of it but the marks (in the browser's storage of this page). A single tile can be marked
   by hand in its menu.
5. Newer pages: **Update the pages** lists the lines to type in the folder of the clones (`git pull` per clone); the
   page itself runs nothing.

## License

[MIT](LICENSE). The page bundles the Svelte runtime; see [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).
