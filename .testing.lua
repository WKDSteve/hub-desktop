-- .testing.lua -- what the test sheets of this repository run against (testing.nvim, docs/SHEETS.md there). The sheets
-- in TESTS/sheets only name an app; how it starts is here, under testing.nvim's trust gate. hub-desktop has no Lua spec.

return {
  sheets = {
    -- the Chrome of the machine, as the e2e tests; playwright-core comes at the version of ui/pnpm-lock.yaml
    browser = { playwright_dir = "ui", channel = "chrome" },
    apps = {
      -- The real binary against a machine of the run, as ui/e2e/fixture.ts makes it: every folder it looks at is in
      -- the run's folder, so it finds no app, no vault and no settings of the person, and NVIM_MACHINE_ROLE=workstation
      -- keeps it from installing releases. The ports are fixed (0 is refused); a port in use makes the hub take a free
      -- one, and the link it prints says which. HUB_DESKTOP_NO_LAUNCH prints the pairing link instead of opening a browser.
      -- The binary embeds ui/dist: build the interface first (cd ui && pnpm build), else the page is a placeholder.
      ["hub"] = {
        build = { "cargo", "build", "-p", "hub-desktop", "--locked" },
        run = { "target/debug/hub-desktop.exe" },
        env = {
          HUB_DESKTOP_CONFIG_DIR = "${temp}/cfg-hub",
          HUB_DESKTOP_PORT = "47391",
          HUB_DESKTOP_WEB_PORT = "47392",
          HUB_DESKTOP_NO_LAUNCH = "1",
          HUB_DESKTOP_BROWSER_LOG = "${temp}/browser.log",
          TASKS_DESKTOP_CONFIG_DIR = "${temp}/cfg-tasks",
          RULES_DESKTOP_CONFIG_DIR = "${temp}/cfg-rules",
          TESTING_DESKTOP_CONFIG_DIR = "${temp}/cfg-testing",
          TASKS_VAULT = "${temp}/vault",
          LOCALAPPDATA = "${temp}/Local",
          APPDATA = "${temp}/Roaming",
          PROGRAMFILES = "${temp}/Program Files",
          USERPROFILE = "${temp}/Home",
          HOME = "${temp}/Home",
          REPOS_DIR = "${temp}/Repos",
          NVIM_MACHINE_ROLE = "workstation",
        },
        ready = { stdout = "^http://127%.0%.0%.1:%d+/#pair=%x+$", as = "link" },
      },
    },
  },
}
