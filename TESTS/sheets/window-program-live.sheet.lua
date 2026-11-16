-- The hand check of the hub as a window program and of the update without PowerShell, in the real profile: the
-- installed Rolling build, its Start menu link and real releases. A Claude session may not install (it sees a
-- redirected AppData and HKCU), so a person does it in a normal terminal. What a machine checks runs elsewhere:
-- tests/window_program.rs (the release build is a window program), tests/cli.rs (a second click opens the page of the
-- running hub, --version, stop ends the hub over the control route), tests/update_route.rs (the update steps, no
-- PowerShell), appkit's tests/window_program.rs (from a terminal the output reaches it) and the sheet link-start-error
-- (an error of a start from a link is a message box).
--   testing sheets step window-program-live
return {
  id = "hub-desktop.window-program-live",
  feature = "hub-desktop.window-program",
  title = "hub-desktop: the installed hub starts from its link without a console, and updates apps without PowerShell",
  task = "hub-desktop/handlauf-hub-als-fenster-programm-und-update-ohne-powershell",
  engine = "manual",
  steps = {
    {
      ask = "In a normal terminal (not a Claude session): hub-desktop from the Rolling build installed (install.ps1 of"
        .. " the README), then started from the Start menu link: no console window comes up and the page opens?",
    },
    { ask = "A second click on the link only opens the page again (still one hub-desktop in the Task Manager)?" },
    {
      ask = "Update of Tasks from the hub: its log shows `$ tasks-hub-windows-x86_64 --version` and `install`, and no"
        .. " powershell; app-proc-watch -ProcessId <pid of the hub> during the update shows no shell and no console"
        .. " window?",
    },
    { ask = "Update of Docmap (an NSIS setup): the setup shows its window and the update waits until it is closed?" },
    { ask = "hub-desktop stop ends the hub at once (not after 10 s), and no hub-desktop.exe is left?" },
  },
}
