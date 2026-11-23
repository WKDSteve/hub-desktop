-- The section Options of the hub on an empty machine of the run (.testing.lua): the switch and the minutes of the CI
-- watcher, a value out of bounds said at the field, a saved value that comes back from the hub, the switch off. What is
-- written into config.json is checked by ui/e2e/hub.spec.ts and the colours by ui/e2e/a11y.spec.ts; this sheet keeps
-- the shots for the judgement whether the section reads well, light and dark. The hub of a sheet is a hub of a test:
-- its watcher never runs (ciwatch::allowed), so the rounds themselves are the hand check of the sheet ci-runner-live.
--   testing sheets step ci-options --headed
return {
  id = "hub-desktop.ci-options",
  feature = "hub-desktop.ci-watcher",
  title = "hub-desktop: the options of the CI watcher, a bad value, a saved value and the switch off, light and dark",
  task = "hub-desktop/abnahme-ci-beobachter-und-runner-im-tray-im-echten-profil",
  engine = "browser",
  app = "hub",
  sizes = { { cols = 1000, rows = 800 } },
  steps = {
    { scheme = "light" },
    { open = "${link}", note = "the pairing link the hub printed" },
    { wait = "document.getElementById('tab-options') !== null", timeout = 10000, note = "paired: the tabs are there" },
    { click = "#tab-options", note = "the tab Options" },
    {
      wait = "document.querySelector('[data-options] input[type=number]') !== null",
      timeout = 10000,
      note = "paired: the section Options is there",
    },
    { expect = "document.querySelector('[data-options] input[type=number]').value === '5'", note = "the default" },
    { expect = "document.querySelector('[data-options] button.save').disabled", note = "nothing to save yet" },
    { shot = "rest-light" },

    { focus = "[data-options] input[type=number]" },
    { press = "Control+a" },
    { press = "0" },
    {
      wait = "document.querySelector('[data-options] input[type=number]').getAttribute('aria-invalid') === 'true'",
      note = "0 is out of bounds",
    },
    { expect = "document.querySelector('#ci-interval-help').textContent.includes('1 to 60 minutes')" },
    {
      expect = "document.querySelector('[data-options] button.save').disabled",
      note = "a bad value is never sent",
    },
    { shot = "bad-value-light" },

    { press = "Control+a" },
    { press = "1" },
    { wait = "!document.querySelector('[data-options] button.save').disabled" },
    { click = "[data-options] button.save" },
    {
      wait = "document.body.textContent.includes('The CI watcher runs every 1 min.')",
      timeout = 5000,
      note = "the hub kept it",
    },
    {
      wait = "document.querySelector('[data-options] button.save').disabled",
      note = "the page shows what the hub says now: nothing changed any more",
    },
    { shot = "saved-light" },

    { click = "[data-options] input[type=checkbox]" },
    { expect = "document.querySelector('[data-options] input[type=number]').disabled", note = "off: no minutes" },
    { click = "[data-options] button.save" },
    { wait = "document.body.textContent.includes('The CI watcher is switched off.')", timeout = 5000 },
    { shot = "off-light" },

    { scheme = "dark" },
    { expect = "matchMedia('(prefers-color-scheme: dark)').matches", note = "the page sees the dark scheme" },
    { shot = "off-dark" },
    { click = "[data-options] input[type=checkbox]" },
    { focus = "[data-options] input[type=number]" },
    { press = "Control+a" },
    { press = "0" },
    {
      wait = "document.querySelector('[data-options] input[type=number]').getAttribute('aria-invalid') === 'true'",
    },
    { shot = "bad-value-dark" },
    { press = "Control+a" },
    { press = "5" },
    { click = "[data-options] button.save" },
    { wait = "document.body.textContent.includes('The CI watcher runs every 5 min.')", timeout = 5000 },
    { focus = "[data-options] input[type=number]" },
    { shot = "focus-dark" },
  },
  judge = {
    "The section Options reads as one block: the switch, the minutes and the button Save, light and dark",
    "A value out of bounds is plainly marked at the field and its help line, light and dark",
    "With the switch off the minutes look disabled, and with the focus a ring around the field is plainly visible",
  },
}
