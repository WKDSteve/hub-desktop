-- TESTS/sheets/features.lua -- the functionalities of hub-desktop as its user meets them; `testing sheets report` says
-- which ones have a feature sheet (`feature = "<id>"`) and in which state, and K16 warns about a sheet that names an
-- id not listed here. Data: read with an empty environment like a sheet (testing.nvim docs/SHEETS.md).
-- The list follows README.md (What it does, Web hub, Hide/order/stop, Update the apps, Testing monitor). hub-desktop
-- has no command surface for `testing surface`, so no entry names one.
return {
  {
    id = "hub-desktop.install",
    title = "Install for the user without administrator rights, with a Start menu link and a desktop link",
    docs = "README.md",
  },
  {
    id = "hub-desktop.tiles",
    title = "Every installed app is a tile; one click starts it (its own hub first) and opens it, native or web",
    docs = "README.md",
  },
  {
    id = "hub-desktop.not-installed",
    title = "Apps that are not installed appear darkened under Install or download, with the release and checksums",
    docs = "README.md",
  },
  {
    id = "hub-desktop.overlay",
    title = "catalog-overlay.json hides and orders tiles; a file that cannot be used is reported and ignored",
    docs = "README.md",
  },
  {
    id = "hub-desktop.stop-app",
    title = "Stop in the menu of a running kit app's tile stops that app",
    docs = "README.md",
  },
  {
    id = "hub-desktop.update",
    title = "Update one app or all of them: the program from its release, checked, and the web variant by git pull",
    docs = "README.md",
  },
  {
    id = "hub-desktop.web-hub",
    title = "hub.html opened from disk lists the web variants, marks the missing ones, and runs nothing",
    docs = "README.md",
  },
  {
    id = "hub-desktop.testing-monitor",
    title = "The Testing section: a light per repository from a testing export, and the red cases of a red run",
    docs = "README.md",
  },
  {
    id = "hub-desktop.button-states",
    title = "A button and a tile of the hub show rest, pointer and keyboard focus apart, light and dark",
    docs = "README.md",
  },
}
