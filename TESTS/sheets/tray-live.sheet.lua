-- The hand check of the icon in the notification area and of the start with the login, in the real profile. The menu
-- (entries, their state in words, what each id does) is tested in src/traymenu.rs; the icon itself, a real login and
-- the login entries are not reachable from a sheet (UI Automation does not open the menu of a notification icon, and a
-- Claude session sees a redirected AppData and HKCU). The one state of the start with the login is the sheet
-- autostart-one-state.
--   testing sheets step tray-live
return {
  id = "hub-desktop.tray-live",
  feature = "hub-desktop.autostart",
  title = "hub-desktop: after a login there is exactly one icon, no console window, and its menu steers the hubs",
  task = "hub-desktop/handlauf-tray-icon-und-autostart-uebernahme-im-echten-profil",
  engine = "manual",
  steps = {
    {
      ask = "In a normal terminal (not a Claude session): hub-desktop from the Rolling build installed"
        .. " (scripts/build-install.ps1 or install.ps1), and hub-desktop autostart status shows the old state?",
    },
    {
      ask = "hub-desktop autostart on names every app it takes over; the Startup folder (shell:startup) holds"
        .. " hub-desktop.lnk and no link of tasks, rules or testing, and config.json of hub-desktop lists them in"
        .. " start_with_hub?",
    },
    {
      ask = "Signed out and in again: exactly one icon (hub-desktop) in the notification area, no console or"
        .. " PowerShell window, and the hubs of start_with_hub are running in its menu?",
    },
    { ask = "Open hub and a left click open the page, paired?" },
    { ask = "Open and Stop of a hub in the menu work on that hub?" },
    { ask = "Quit hub takes the icon away and leaves the other hubs running?" },
    { ask = "Optional: app-proc-watch -ProcessId <pid of hub-desktop tray> for a minute shows nothing it started?" },
  },
}
