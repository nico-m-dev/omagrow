# OmaGrow — repository guidance

OmaGrow is a standalone Omarchy app for tracking plants, gardens and grows. We build it step by
step, together.

## How we work

- Discuss first, then build. For a structural choice (data model, modules, dependencies), propose
  it in a few sentences and wait for agreement; no large refactor without a yes.
- Small working steps. One screen that works beats three that are half done.
- Do not build options "for later". Add capability when it is needed.
- Ask when unsure; do not guess what the user means.
- Documentation is part of the change. Every fact has one owner: update that document and link to
  it from elsewhere. Do not copy a rule into a second document; if two docs disagree, the owner is
  right and the other gets corrected.
- Before claiming something works, run it. Static checks are not evidence of live behaviour.

## Documentation

The map is in [README.md](README.md#documentation). In short:

- `README.md` — what the app is, requirements, build and a short status.
- `docs/roadmap.md` — the single list of what is built, what is next, what is deferred and what is
  still open.
- `docs/plant-data.md`, `docs/plantings.md`, `docs/weather.md`, `docs/history.md`,
  `docs/media.md`, `docs/units.md`, `docs/agent.md`, `docs/development.md` — one subject each;
  behaviour belongs there.
- `AGENTS.md` — how we work, safety and this map. No status and no second decisions list.

If behaviour changes, update the owning document. If the plan changes, update the roadmap. Do not
keep a second status list here.

## Non-negotiables

- Rust, native GTK4/libadwaita app — not a TUI and not a shell plugin. UI, code, data and
  documentation in English.
- Local SQLite data. Profiles are user-owned; there is no bundled world database and no plant API.
- Agents use the CLI and the app's validation; they never write SQLite directly. Values are
  `estimated` with a source; `confirmed` and `not_applicable` stay protected. Details in
  `docs/agent.md` and `docs/history.md`.
- Physical actions stay outside a release. The user's automation controls the equipment and an
  agent never runs hardware. Actuator control is a future experiment only; see `docs/roadmap.md`.
- No tokens, API keys or personal data in the repository; keys belong in the OS keychain.
- Follow existing patterns and native GTK conventions.

## Safety and the knowledge base

- Do not write outside this repository, except the knowledge-base notes the machine-wide
  `AGENTS.md` allows (the journal line and, on request, a daily-check sheet).
- Follow the machine-wide `AGENTS.md`: one journal line per session when there is something to
  report. Large work gets a temporary checklist in `docs/TASKS.md` and a subagent review before it
  is called done; small fixes do not.
- Background notes live outside the repository and are not a status or worklog. If they disagree
  with these docs, the repository is right and the note gets corrected.
