-- The hand check of the CI watcher and of the runners in the tray, in the real profile. A hub of a test never runs the
-- watcher and never asks runners.ps1 (ciwatch::allowed), and a test never touches the real runners, so the rounds, a
-- verdict in a real task and the start of a runner are not reachable from a sheet. The logic is tested in
-- tests/ciwatch.rs and tests/runners.rs (fake tasks CLI, fake testing, fake pwsh), the section Options in the sheet
-- ci-options and in ui/e2e.
--   testing sheets step ci-runner-live
return {
  id = "hub-desktop.ci-runner-live",
  feature = "hub-desktop.ci-watcher",
  title = "hub-desktop: the CI watcher writes a verdict into a real task, a waiting runner starts from the tray",
  task = "hub-desktop/abnahme-ci-beobachter-und-runner-im-tray-im-echten-profil",
  engine = "manual",
  steps = {
    {
      ask = "In a normal terminal (not a Claude session): hub-desktop at 5eb8aeb or later installed"
        .. " (Rolling build or scripts/build-install.ps1) and its hub running (icon in the notification area)?",
    },
    {
      ask = "On the hub page, tab Options: switch on, 5 minutes; after a little more than a minute the tab"
        .. " Testing says 'CI watcher: last round ...' (not 'does not run')?",
    },
    {
      ask = "A task with ci: pending and a commit whose hosted CI is green: after the next round it has ci: green,"
        .. " and its history names --by=hub-desktop?",
    },
    {
      ask = 'Minutes set to 1 and saved: config.json has "ci_watch_interval_min": 1, and hub.log shows a round'
        .. " ('ci watch: ...') every minute without a new start;"
        .. " the switch off: 'does not run: config.json turns it off'?",
    },
    { ask = "The tray menu Runners lists every runner of runners.ps1 status all as running or not running?" },
    {
      ask = "A runner stopped (runners.ps1 stop <repo>) and a push into its repository: after the next round the"
        .. " menu says 'a push waits'; a click starts it (runners.ps1 status <repo>: local=running github=online)"
        .. " and the run starts?",
    },
    {
      ask = 'With "runners": { "start_when_waiting": true } in config.json the same case starts the runner'
        .. " without a click?",
    },
    { ask = "No PowerShell or console window opened during all of this?" },
  },
}
