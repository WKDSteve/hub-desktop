# Programs hub-desktop starts

Every program `hub-desktop` starts, with why, how long it lives and which way of
[appkit's `docs/PROCESSES.md`](https://github.com/WKDSteve/appkit/blob/main/docs/PROCESSES.md)
(rule `RS-08`) starts it. **A process that is not in this list is a bug.** The list is taken
from the code (`tools/process-starts.allow`, the `appkit_core::child` and `launch` calls of this
repository and of appkit) and checked against a measurement with `app-proc-watch`.

The ways:

- **child**: `appkit_core::child`. No console window, the streams are pipes of the hub, bound to
  the hub by the system (a job object on Windows, a process group elsewhere): it ends with the
  hub, also when the hub crashes or is killed, together with everything it started. Its start
  is in `hub.log`.
- **launch**: `appkit_core::launch::spawn` (`spawn_with` when a variable is handed over). The
  program of an app is not the hub's: it gets its own window where it has one and outlives the
  hub on purpose; nothing of the hub is inherited.
- **system**: the system opens something (`ShellExecuteW` on Windows); no process of the hub is
  started. On macOS and Linux `/usr/bin/open` or `xdg-open` is started through **launch**.

| Program | Why | How long | Way |
| --- | --- | --- | --- |
| `hub-desktop tray` (Windows; elsewhere `hub-desktop serve`) | `start` (the link, a double click, the login entry, `install`) puts the hub in the background, with its icon in the notification area | until **Quit hub** in the menu of the icon, `stop`, or the hub ends itself | named exception: the hub in the background (`background::start_as(.., "tray", Program::Window)`: a window program, a process group of its own, nothing hidden, output to `hub.log`); the icon is drawn by the hub's own process (`tray-icon`), no program and no WebView |
| `<hub of an app> start --open` (tasks-, rules-, testing-hub, pult) | the click on the tile of an app of the kit, **Open** in the menu of the icon | the call ends at once; the hub it starts is the app's and outlives hub-desktop | launch |
| `<hub of an app> start` with `<APP>_STAY=1` | at the login, for each app in `start_with_hub` of `config.json` (while hub-desktop itself starts with the login) | as above; the hub stays without a page | launch |
| the program of an app with a window (docmap-desktop, streamtap, pult, takt) | the click on its tile | the app's | launch, its own window |
| ai-desktop (a server in its console) | the click on its tile when it does not run | until you close its console | named exception: a server in its console (`Window::NewConsole`), visible; its page then opens in the browser |
| `<hub of an app> stop` | **Stop** in the menu of its tile or of the icon | the call | launch |
| `git` (on `PATH`) | update of a web variant: `symbolic-ref`, `status`, `rev-parse`, `pull --ff-only` | the call; at most 10 minutes | child |
| `nvim` (on `PATH`) with `$REPOS_DIR/tasks.nvim/scripts/tasks.lua` or `$REPOS_DIR/testing.nvim/scripts/testing.lua` | the CI watcher: `tasks list --done --ci=pending,red`, `tasks call finished`, `tasks set <id> ci=green\|red --by=hub-desktop`, `testing ci remote --json [--sha=<commit>] <checkout>`; also the runners' look at a push that waits | the call; at most 90 s | child |
| `git` (on `PATH`) | the CI watcher: `cat-file -e <head>^{commit}`, `merge-base --is-ancestor` in a checkout below `REPOS_DIR` | the call; at most 90 s | child |
| `pwsh -NoProfile -NonInteractive -File $NVIM_CONFIG_DIR/scripts/runners.ps1` (`pwsh` on `PATH`) | the runners in the menu of the icon (private machine only): `status all` in the rhythm of the CI watcher, `ensure <repo>` on a click on a stopped runner or, with `runners.start_when_waiting`, for a push that waits | the call; `status` at most 2 minutes, `ensure` at most 4 minutes; the runner `ensure` starts is started detached by the script (outside the hub's job) and outlives the hub on purpose | child; the one PowerShell the hub runs, so that the runners have one source of truth |
| `gh` (on `PATH`) | update of a program: `release download`, `attestation verify` | the call; at most 10 minutes (the attestation 1 minute) | child |
| the checked file of a release | update of a program: `--version` and `install` of a program of the kit, or the setup with its window | the call; what it starts on purpose (the hub of the app, the app after a setup) stays | child (`Leftovers::Keep` for what it started on purpose) |
| `<installed hub-desktop> start` | `hub-desktop install` brings the hub back from the installed program | the call; the hub it puts in the background stays | child (`background::start_from`) |
| the default browser | the page of the hub, the page of an app (also a web variant served from its clone) | yours | system (`appkit_hub::browser::open`) |
| `conhost.exe` | Windows gives every console program (`git`, `gh`, a setup that is one) a console host, also without a window | as long as its console program | comes with each child; no window |

When an update call runs out of time, it ends together with everything it started; nothing of
an update is left running. No PowerShell and no script of a release is run for an update: the hub
runs the checked file directly. The one script the hub runs is `runners.ps1` of the nvim config
(above), never one of a release or of a request.

What the hub does **not** start any more: `powershell.exe` (the update), `taskkill.exe` (`stop`
asks the hub over its control route), `reg.exe` (the login entry and the uninstall entry go
through the registry API), `icacls.exe`, `cmd.exe` or a script host.

The release build is a window program (GUI subsystem): the link, the login entry and a double
click open no console window.
