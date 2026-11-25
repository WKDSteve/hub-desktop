-- Sign out of the hub's page: the button in the bar ends the session of the tab at the hub, and the page goes to the
-- state that asks for a link, on an empty machine of the run (.testing.lua, app "hub"). The unit tests and
-- ui/e2e/hub.spec.ts check the same through the code; this sheet stays with the repository and runs again with every
-- change, and leaves the shots for the judgement of how the button looks in the bar.
--   testing sheets step sign-out --headed
return {
  id = "hub-desktop.sign-out",
  feature = "hub-desktop.sign-out",
  title = "hub-desktop: Sign out in the bar ends the session and the page asks for a link",
  task = "ALL/abnahme-abmelden-knopf-in-rules-testing-und-hub-desktop-im",
  engine = "browser",
  app = "hub",
  -- one size: the pairing link of the app is good for one pairing, and the app runs once for all sizes
  sizes = { { cols = 1000, rows = 700 } },
  steps = {
    { scheme = "light" },
    { open = "${link}", note = "the pairing link the hub printed" },
    { wait = "document.querySelector('[data-tile=\"tasks\"]') !== null", timeout = 10000, note = "paired: the tiles are there" },
    { expect = "(localStorage.setItem('hub-desktop.lang', 'en'), setTimeout(() => location.reload(), 0), true)", note = "the page in English, whatever the language of the browser" },
    { wait = "document.documentElement.lang === 'en' && document.querySelector('[data-tile=\"tasks\"]') !== null", timeout = 10000, note = "reloaded in English" },
    { expect = "/^[0-9a-f]+$/.test(sessionStorage.getItem('hub-desktop.session') ?? '')", note = "the tab holds a session" },
    { expect = "[...document.querySelectorAll('header button')].some(b => b.textContent.trim() === 'Sign out')", note = "the button is in the bar" },
    { shot = "bar-light" },
    { scheme = "dark" },
    { shot = "bar-dark" },
    { hover = 'role=button[name="Sign out"]' },
    { expect = "[...document.querySelectorAll('button')].find(b => b.textContent.trim() === 'Sign out').matches(':hover')" },
    { shot = "hover-dark" },
    { click = 'role=button[name="Sign out"]', note = "the person signs out" },
    { wait = "[...document.querySelectorAll('h2')].some(h => h.textContent.trim() === 'Open the hub with its link')", timeout = 10000, note = "the page asks for a link" },
    { expect = "sessionStorage.getItem('hub-desktop.session') === null", note = "the token is gone from the tab" },
    { expect = "![...document.querySelectorAll('button')].some(b => b.textContent.trim() === 'Sign out')", note = "no button without a session" },
    { expect = "document.querySelector('[data-tile]') === null", note = "no tile of the machine is shown" },
    { shot = "signed-out" },
  },
  judge = {
    "Sign out sits in the bar, is readable in the light and the dark scheme, and its border shows against the bar",
    "After the click the page says it has no session and shows nothing of the machine",
  },
}
