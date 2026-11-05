# OmaGrow

A plant, garden and grow journal for Omarchy — one app for what you grow, what it needs and what
actually happened. The app follows the active Omarchy theme and stores everything locally in
`~/.local/share/omagrow/omagrow.db` (SQLite, WAL).

> **Work in progress.** This is an early, unreleased app. The screens, the data model and this
> documentation can still change, and there is no package or stable release yet.

## Requirements

- Rust 1.85+ (edition 2024)
- GTK 4.12+ and libadwaita 1.5+
- SQLite 3 (linked with `libsqlite3`)
- Omarchy, for the theme palette and the `ttf-ia-writer` font used for the wordmark
  (falls back to Adwaita Sans when missing)

## Build and run

```sh
cargo run
```

See [docs/development.md](docs/development.md) for the edit-run loop, the GTK inspector and how to
check theme following while the app is open.

## What works today

- **Plant profiles**: create, edit, duplicate and delete; stages, requirements, timing, traits and
  attributes; provenance chips, cover photo, completeness and Activity/History with revert.
- **Plantings and spaces**: setup, finish/reopen, a derived timeline and month calendar, placement
  history, avatars, and a diary with water, feed, prune, repot, pest, disease, notes, photos,
  harvest, support and stage starts.
- **Readings and climate**: manual readings, Open-Meteo current values plus 92 days of history with
  a call budget, and one Climate section for temperature, rain, sunshine and humidity, including
  day/night.
- **Settings**: place search, a frost estimate, the Open-Meteo switch, the weather budget, the
  agent-source policy and metric/imperial display.
- **Agent CLI**: read profile data as JSON, create, update or copy profiles, and write validated
  values or proposals through the app's own rules.

The full status, release scope and open items live in [docs/roadmap.md](docs/roadmap.md).

## Agent access

The same binary carries a CLI for coding agents:

```sh
omagrow agent guide
omagrow profile list
omagrow profile show 5
omagrow set 5 --check < values.json
```

The CLI shares the app's validation and never writes SQLite directly. The contract, JSON shape,
proposals and write rules are in [docs/agent.md](docs/agent.md); the change ledger is in
[docs/history.md](docs/history.md). The app never launches an agent.

## Tested with

- GTK 4.24.1, libadwaita 1.10.0
- Omarchy 4.0.0.alpha (Hyprland), theme zomarchy
- Rust 1.99.1

## Documentation

| Document | Contents |
| --- | --- |
| [docs/roadmap.md](docs/roadmap.md) | What is built, what comes next, what is deferred and what is still open |
| [docs/plant-data.md](docs/plant-data.md) | Profiles, stages, requirements, timing, attributes, provenance and schema |
| [docs/plantings.md](docs/plantings.md) | Plantings, spaces, placements, the diary and local readings |
| [docs/weather.md](docs/weather.md) | Open-Meteo, sun times, the weather sync and the climate graphs |
| [docs/history.md](docs/history.md) | The change ledger and revert |
| [docs/media.md](docs/media.md) | Photos, avatars, illustrations and Help screenshots |
| [docs/units.md](docs/units.md) | Units and the metric/imperial preference |
| [docs/agent.md](docs/agent.md) | The CLI and the agent contract |
| [docs/development.md](docs/development.md) | Build, UI conventions and tests |
| [AGENTS.md](AGENTS.md) | How we work in this repository |
