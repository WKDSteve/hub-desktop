-- A start from the link that fails: the release build is a window program, so nobody reads its stderr there and the
-- error must come as a message box (appkit console::report). The app `link-start-error` of .testing.lua starts it as
-- a link does, against a config.json it cannot use; the message box is its only window. What stays for a person: the
-- wording (it should name the cause) and the start from the real Start menu link (manual sheet window-program-live).
--   testing sheets run --sheet link-start-error
return {
  id = "hub-desktop.link-start-error",
  feature = "hub-desktop.window-program",
  title = "hub-desktop: a start from a link that fails says so in a message box",
  task = "hub-desktop/handlauf-hub-als-fenster-programm-und-update-ohne-powershell",
  engine = "desktop",
  app = "link-start-error",
  sizes = { { cols = 640, rows = 240 } },
  steps = {
    { wait = "OK", timeout = 5000, note = "the message box of hub-desktop is up, with its button" },
    { shot = "message" },
    { click = "OK", note = "the person closes it" },
  },
  judge = { "The message says why the hub did not start and where to read more (hub.log)" },
}
