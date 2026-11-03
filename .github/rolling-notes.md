**This is not a release.** It is the newest state of `main`, rebuilt after every green CI run and replaced each time. Nothing here is signed or tested by hand; use a release for anything that matters.

**Windows only** (x86-64). The files of this page are public: no GitHub login is needed.

**Install it with the script** (for the current user, no administrator rights, and no SmartScreen stop: the script downloads the program itself and checks it against `SHA256SUMS`). In your own PowerShell (not elevated):

```
powershell -ExecutionPolicy Bypass -c "irm https://github.com/WKDSteve/hub-desktop/releases/download/latest/install.ps1 | iex"
```

Or save `install.ps1` from this page and run `powershell -ExecutionPolicy Bypass -File .\install.ps1` (a plain `.\install.ps1` is blocked: by the execution policy, or by the mark a browser puts on a downloaded file). To add a link on the desktop, run it as a script block in a PowerShell session instead: `& ([scriptblock]::Create((irm https://github.com/WKDSteve/hub-desktop/releases/download/latest/install.ps1))) -InstallArgs '--shortcut'`.

Run it again to update to the newest rolling build.

Or take the file yourself: `hub-desktop-windows-x86_64.exe`. A double click runs the hub in its window and opens the browser (a second click finds the running hub). Windows SmartScreen warns about an unsigned program: "More info", then "Run anyway". `uninstall.ps1` removes it again; `SHA256SUMS` holds the checksums of all files.

The hub lists the other apps (`tasks-desktop`, `rules-desktop`, `testing-desktop`, `docmap-desktop`, `ai-desktop`) and starts the ones that are installed; it needs none of them to run. `hub-desktop --version` names the commit this file was built from; `hub-desktop stop` ends the hub.
