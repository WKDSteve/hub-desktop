# hub-desktop (Rust-Hub + Svelte-Oberfläche: der Einstieg in alle eigenen Apps)

Der Hub listet die anderen `*-desktop`/`*-web`-Apps als Kacheln und startet sie mit einem Klick. Er hält keine
Daten und keine Engine: Dummy-Runtime auf `appkit-hub`, Routen unter `/api/hub/`, Seite eingebettet. Konzept:
`$REPOS_DIR/tasks-vault/ALL/Hub-Desktop-Konzept-2026-10-31.md`, Plan und Tasks: `tasks plan
--plan=hub-desktop/hub-desktop-vom-launcher-zum-hub`. Natives Bauen und Testen nur auf dem privaten Rechner
(`not-on-workstation`).

CI und Release laufen bei `push` auf dem selbst gehosteten Windows-Runner, bei `pull_request` auf `windows-latest`
(nie ein Pull Request auf dem eigenen Runner, kein `pull_request_target`; siehe Kopf von `.github/workflows/ci.yml`).

## Bauen und prüfen (vor "fertig")

```
cd ui && pnpm install --frozen-lockfile && pnpm check && pnpm test && pnpm build
touch crates/hub-desktop/src/ui.rs        # sonst bettet das Binary die alte Oberfläche ein
cargo build --locked
cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python tools/check-control-chars.py
python tools/check-color-literals.py      # keine Farbe außerhalb der Tokens von appkit-ui
python tools/check-appkit-pin.py
python tools/check-lock-age.py --min-days 7
```

- `pnpm build | tail -0` bricht den Build ab (SIGPIPE): `tail -1` nehmen.
- Ein Fix ohne Test, der ohne den Fix rot wird, gilt nicht als fertig. Nach der Sabotage neu bauen (`touch` +
  `cargo build`), sonst bleibt der Test grün.
- `Cargo.lock`: `check-lock-age.py` verlangt Crates, die älter als 7 Tage sind. Meldet es eine zu junge Version:
  `cargo update -p <crate> --precise <ältere Version>`.

## Nie aus einer Claude-Sitzung

- **`hub-desktop install`, `start`, ein Autostart** und dasselbe der Geschwister (`tasks-hub install|start|autostart`)
  nicht ausführen: Die Claude-App leitet AppData und HKCU um, ein Lauf würde die falschen Orte verändern. Das Binary
  nur gegen einen Fake-Ordner starten (`HUB_DESKTOP_CONFIG_DIR`, `HUB_DESKTOP_PORT`, `HUB_DESKTOP_NO_LAUNCH=1`, siehe
  `crates/hub-desktop/tests/cli.rs`); Tests mit Ordnern über `appkit_scratch::Scratch`, nie gegen den echten Rechner.
- **Runner-Guard:** Ein Push-Lauf auf dem self-hosted Runner wird abgelehnt, bis der Nutzer den Commit freigibt. Nie
  selbst freigeben; dem Nutzer die Befehle nennen:
  `nvim -n -i NONE --headless -u NONE -l E:/repos/testing.nvim/scripts/testing.lua ci trust WKDSteve/hub-desktop <sha>`,
  dann `gh run rerun <id> -R WKDSteve/hub-desktop`. Vorher `pwsh $env:NVIM_CONFIG_DIR\scripts\runners.ps1 ensure
  hub-desktop`.

## appkit (der geteilte Unterbau)

- Wächter, Pairing, Installation, Kindprozess-Start (`launch::spawn`), Browser-Öffner (`browser::open`) und die
  Sicherheitsmatrix liegen in [appkit](https://github.com/WKDSteve/appkit) (`E:/repos/appkit`), eingebunden per Tag in
  `[workspace.dependencies]` von `Cargo.toml` und in `ui/package.json` (`appkit-ui`, `appkit-web`). Eine Änderung dort:
  erst in appkit (CI grün, neues Tag nach dessen `docs/RELEASING.md`), dann hier das Tag heben (`Cargo.toml`,
  `ui/package.json`, später die `uses: WKDSteve/appkit/release-kit/...@<sha>`), `cargo update -p appkit-hub`,
  `cd ui && pnpm install`, `python tools/check-appkit-pin.py`. `python tools/bump-apps.py` in appkit macht das für die
  Apps (der Hub gehört dort noch hinein: Task `appkit/tools-bump-apps-py-um-hub-desktop-erweitern`).
- Gleichzeitig an appkit und hier arbeiten: `[patch."https://github.com/WKDSteve/appkit"]` in einer **nicht
  eingecheckten** `.cargo/config.toml`; nie einen Pfad in `Cargo.toml` committen. Ein `[patch]` schreibt `Cargo.lock`
  um: danach `git checkout -- Cargo.lock`.

## Aufbau

- `crates/hub-desktop/src/`: `lib.rs` (`IDENTITY`: App `hub-desktop`, Binary `hub-desktop`, Port 17610, `route_ns` `hub`),
  `server.rs` (Dummy-Runtime, Routen, `DATA_ROUTES` für die Matrix), `app.rs` (`serve`, `open`, Doppelklick),
  `background.rs` (`stop`/`install`/`uninstall` aus dem Kit), `ui.rs` + `build.rs` (die gebaute Oberfläche im Binary).
- Ohne Argument läuft der Hub im Vordergrund (sichtbare Konsole; der Startmenü-Link startet ihn minimiert) und öffnet
  den Browser gepairt; ein laufender Hub wird nicht ein zweites Mal gestartet. Kein `CREATE_NO_WINDOW`, kein versteckter
  Hintergrundprozess als Standard (EDR).
- Neue Route: in `server::routes` **und** in `DATA_ROUTES`, sonst probiert die Matrix sie nie. Jede Route unter `/api/`
  braucht eine Sitzung, bis sie bewusst geöffnet wird (nur `hello` ist offen).
- Der Start anderer Apps nur per argv über `appkit_hub::launch::spawn`, Programmpfade nur aus der Erkennung, nie aus
  einem Request oder aus `config.json`; Single-Flight je App (Konzept §6, §8).
- Ports: 17610 Hub, 17611 Web-Listener, 17618 testing-hub; die Liste steht in `appkit-testkit/src/ports.rs`.

## Svelte 5 und Browser-Tests

- Ein `$effect` trackt alles, was der Aufruf in ihm liest: Aufrufe, die selbst Zustand lesen, gehören in
  `untrack(() => ...)`. Unit-Tests sehen keine `$effect`; was ein Effekt tut, gehört in den Browser-Test (Anfragen
  zählen, nicht nur das Ergebnis ansehen).
- Nach `pnpm build` ist das eingebettete Binary alt: `touch crates/hub-desktop/src/ui.rs`, dann bauen, dann testen.
- Neue Abhängigkeiten nur nach Rückfrage; Rust-Library-Code ohne `unwrap()`/`expect()` (Fehler über `Result`).
