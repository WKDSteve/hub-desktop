hub-desktop as a program for Windows: the hub that starts the other apps, with its interface inside it.

## Install

Without administrator rights, for one user. The `install.ps1` of this release installs exactly this release: it
downloads the archive, checks it against `SHA256SUMS` and hands over to `hub-desktop install`. No GitHub login is
needed: run it in your own terminal (PowerShell not elevated):

```
powershell -ExecutionPolicy Bypass -c "irm https://github.com/WKDSteve/hub-desktop/releases/download/{tag}/install.ps1 | iex"
```

Or save `install.ps1` from this page and run `powershell -ExecutionPolicy Bypass -File .\install.ps1` (a plain
`.\install.ps1` is blocked: by the execution policy, or by the mark a browser puts on a downloaded file).

To register the hub with your login and add a link, pass the arguments; a pipe into `iex` takes none, so run the
script as a script block in a PowerShell session:

```
& ([scriptblock]::Create((irm https://github.com/WKDSteve/hub-desktop/releases/download/{tag}/install.ps1))) -InstallArgs '--autostart','--shortcut'
```

The program downloaded by the script is not stopped by SmartScreen; what is checked instead is that it is the file
`SHA256SUMS` of this release lists.

Run the same script again to repair this release (to update, take the script of a newer release); remove the program
with `uninstall.ps1` (your settings stay unless you add `-PurgeSettings`; your vault is never touched). The install
script comes from the release-kit of appkit, the uninstall script from `scripts/` at this tag; both are listed in
`SHA256SUMS`.

## Files

| System | Archive |
| ------ | ------- |
| Windows (x86-64) | `hub-desktop-<version>-x86_64-pc-windows-msvc.zip` |

There is no Linux or macOS build any more: the release is made on the author's Windows machine.

`SHA256SUMS` lists the checksum of every file (archive and scripts):

```
sha256sum -c SHA256SUMS --ignore-missing
```

The program is not signed yet: Windows may show SmartScreen. The script does not need that.
