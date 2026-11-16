-- The hand check of the start with the login (decision 15 of the plan
-- ALL/keine-sichtbaren-shell-und-hintergrundprozesse-in-den-apps):
-- the tile here and the app itself show and switch one state, hubs that hub-desktop starts at the login stay.
-- What a machine can check runs in the tests with a fake login and folders of their own (appkit loginstate,
-- tests/autostart_route.rs, testing-desktop tests/autostart.rs, tests/cli.rs for the hub left alone). What is left
-- needs the real login entries, the real Startup folder and a real login: a person, in a normal terminal (a Claude
-- session sees a redirected AppData and HKCU), with installed builds on appkit v0.60.0 or newer.
--   testing sheets step autostart-one-state
return {
  id = "hub-desktop.autostart-one-state",
  feature = "hub-desktop.autostart",
  title = "hub-desktop: the start with the login is one state in the tile and in the app, hubs started with it stay",
  task = "hub-desktop/handlauf-autostart-in-kachel-und-app-zeigt-denselben-stand-h",
  engine = "manual",
  steps = {
    {
      ask = "hub-desktop autostart on: autostart status names the apps taken over, the Startup folder (shell:startup)"
        .. " holds hub-desktop.lnk and none of the apps, and HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run"
        .. " holds no value of hub-desktop, tasks-desktop, rules-desktop or testing-desktop any more?",
    },
    {
      ask = "Tile tasks, menu Start at login off: tasks-hub autostart status says off, and the Options menu of the"
        .. " page of tasks shows the switch off (after the window got the focus)?",
    },
    {
      ask = "Switched on again in the page of tasks: after r (rescan) the tile says Starts at login, and config.json"
        .. " of hub-desktop lists tasks in start_with_hub?",
    },
    {
      ask = "The same once with testing (Help menu: Start with the login; the tick is right after a change of"
        .. " window) and with rules (Help menu)?",
    },
    {
      ask = "pult: the switch of its tile writes and removes pult.lnk in the Startup folder (not the list)?",
    },
    {
      ask = "hub-desktop autostart off: the apps of the list have their own link in the Startup folder again (the"
        .. " line: starts with your login on its own again), and their switches still show on?",
    },
    {
      ask = "With hub-desktop autostart on, signed out and in again: the hubs of start_with_hub still run more than"
        .. " ten minutes later with no page open and no paired phone (<app>-hub open finds them)?",
    },
  },
}
