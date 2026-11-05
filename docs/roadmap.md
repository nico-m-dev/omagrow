# Roadmap

Where OmaGrow is going: what is built, what comes next, what is deliberately deferred and what is
still open. This is the single status and plan document. The topic documents describe behaviour and
link here for what is not built.

## The goal

A plant profile holds what a variety needs. A planting holds what actually happened: dates,
observations, photos, sensor readings, and the actions taken. From that, OmaGrow can show a
calendar and notifications, and an agent can help run a grow: watch the data, notice that the soil
has been wet for days, and suggest a shorter watering cycle. Physical work stays with the user.

The app owns the data and the rules. The agent is a client with a narrow scope, not a second
owner of the database.

## What exists

| Layer | State |
| --- | --- |
| Plant profiles and plant data | Built: editors, provenance, completeness and history; see [plant-data.md](plant-data.md) and [history.md](history.md) |
| Agent CLI and proposals | Built: reads, create/update/copy, validated writes and proposals; see [agent.md](agent.md) |
| Plantings, spaces and diary | Built: setup, timeline, calendar, placements, diary, avatars; see [plantings.md](plantings.md) |
| Readings and weather | Built: manual readings, Open-Meteo history with a budget, climate graphs; see [weather.md](weather.md) |
| Settings and units | Built: location, frost estimate, Open-Meteo switch, weather budget and units; see [weather.md](weather.md) and [units.md](units.md) |
| Interface and Help | Built: sidebar, wide-window rail, Help overview, screenshots and credits; see [development.md](development.md) |
| Notifications and Warnings | Not yet; first release |
| Agent commands for plantings and observations | Not yet; first release |
| Sensors | After the first release; readings only |
| Visual space builder | After the first release |

## Release scope

The first release is a usable local app, not a package and not a frozen data model. It adds
Notifications and Warnings (per planting, chosen by the user) and agent commands for plantings and
observations. Sensors (readings only), the visual space builder and packaging come after it. The
order below is the plan.

## Plan rules

The detailed rules for agent writes, history, media, timing, weather and placements live in their
own documents. Only the decisions that shape the release plan are repeated here.

1. **Physical actions stay outside a release.** A release never runs a pump or a light, and an
   agent never controls hardware directly. The user's own automation stays in charge; the app only
   records what happened. Actuator control is a future experiment, not a release feature; see
   [Future experiments](#future-experiments).
2. **The user sets the notifications.** Notifications and warnings are configured per planting, not
   as one global feed. A message names its source — a reading, the weather model or the diary — and
   never mixes measured and model data in one sentence.

## Order

The build through phase 4f is complete; the topic documents describe it. What remains:

**First release**

1. **Notifications and Warnings.** Stage-aware and source-aware: local readings against the
   requirement bands, weather only during outdoor placements, and a clear model label. A
   weather-only warning and a reading-only warning stay distinguishable, and neither mixes the
   sources in one sentence. The user decides per planting what to be told and how; this is not one
   global notification feed.
2. **Agent commands for plantings and observations.** The grow layer joins the CLI with the same
   validation, history and scope rules as the profile commands: plantings, diary entries, readings
   and the setup that belongs to them. An agent still never writes SQLite directly.

**After the first release**

3. **Sensors and camera.** Sensor readings join `climate_readings`, and camera frames become
   observations with a photo. Most sensors already have their own automation; OmaGrow only receives
   the readings and never takes the equipment over. The agent may suggest an adjustment with a
   reason, but the user acts on it.
4. **The visual space builder.** A layout of an indoor or greenhouse space with the plants, fans
   and other equipment where they stand. For an outdoor space, the stored coordinates could later
   calculate shadows.
5. **Packaging and a wider release.** A desktop entry and icon, so the app can be launched outside
   the terminal.
6. **Future experiment, not a release: an agent that runs a grow.** See
   [Future experiments](#future-experiments).

MCP comes when a host benefits from tool discovery or per-tool approval; the CLI stays the contract.

## Not built yet

These are the deliberate gaps. They are not promises with a date; the order above says what comes
first.

### Profiles and plant data

- **Starter templates.** A new profile starts empty. Lifecycle presets (annual from seed, woody
  perennial, houseplant, cannabis, …) are not built.
- **Profile export.** A profile cannot be exported as one readable JSON document for backup or
  sharing. The CLI read shape is close, but importing that shape is not built. Proposals have
  their own validated import.
- **Requirement inheritance in the UI.** The store resolves a stage row over a profile default and
  honours `not_applicable`, but the profile page shows the rows as stored instead of using that
  resolution.
- **Custom attribute categories.** A custom key picks one of the fixed categories; users cannot
  create or reorder a category.
- **Attributes beyond the profile.** A definition can be marked `planting` or `observation`, but no
  value can be written there yet, and the custom-key form always creates a profile key.
  Stage-scoped keys can only be created through the store API.
- **JSON attribute values.** The `json` type is stored as text and is not parsed or validated.
- **Stage editing.** Reordering is arrow-based; drag-and-drop and inserting a stage between two
  others in one gesture are not built.
- **Changing a requirement's key.** Editing a requirement keeps its stage, metric and day part
  fixed; another key means deleting the row and adding a new one.
- **Provenance changes in the ledger.** Confirming, unconfirming and marking not applicable update
  the chip but do not create a history row. There is also no separate "reviewed source" step: a
  confirmed value keeps the agent's source.
- **Warnings about the model itself.** The app does not warn about chill hours on an annual, a
  short-day plant without a critical day length, or a minimum container volume without container
  suitability.
- **Manual UI pass.** The core flow has been checked on live profiles, but a final pass of every
  editor edge case is still worthwhile before a release.
- **Design fields without a home.** These appear in older design notes but are neither columns nor
  registered attributes today: `plant_types`, human and pet toxicity, thorns, invasive status,
  hardiness zones, altitude range, `npk_ratio`, a separate germination-days field (the germination
  stage duration is the current path), pollination self-fertility and vector, hygiene notes, and a
  disorder list. Add them as an attribute or a typed field when a screen needs them.

### Grow layer

- **Sensors are not just another reading source.** The readings table and card are ready, and the
  space rail carries a **Sensors** placeholder. The first real sensor data should check the dot
  drawing, because a fill on each dot clears the accumulated line path. A sensor and its readings
  belong to the space; a planting gets its own sensor card only when per-plant soil sensors arrive,
  and that is planting-owned data, not a summary of the space.
- **Light start hint.** A space with LED or HPS hours but no start time has no day/night boundary.
  The graph shows none rather than a guess; a passive hint in the setup form is not built.
- **Global calendar.** Each planting has its own month calendar; one calendar across plantings is
  not built.
- **Aggregation beyond 90 days.** The range views already collapse a day into daily values. A much
  longer history would need weekly or monthly grouping; `weather_hours` keeps 92 days, while older
  days keep their daily aggregates.
- **A fifth climate metric.** The section has a seam for another metric (a label, unit, axis, source
  list and guide key), but no sensor metric exists yet. Rain and sunshine stay weather-only.
- **Succession and yield analysis.** A `succession` timing rule exists, but automatic repeated
  sowings, succession planning and yield analysis are not built.
- **Diary editing.** Entries can be created and deleted, not edited. A repot delete leaves the
  current pot size. There is one photo per entry, a stage start claims one occurrence per cycle, a
  second flush within the same cycle is a diary entry rather than a second timeline segment, and a
  harvest prompt on finish does not exist.
- **Manual tasks and reminders.** A separate task list is not planned. Logging what was done is the
  diary; a reminder belongs to Notifications and Warnings, where the user decides per planting.
- **Space nesting.** A garden containing a greenhouse is not modelled; a nullable parent on
  `spaces` can be added without breaking the flat list.
- **Typed shared setup.** Irrigation, fans and CO2 are notes today; add a typed column when a
  screen uses it.
- **Scoped change ledger.** History is profile-only; planting setup and the diary are not in it. A
  planting-scoped ledger with revert can follow if setup changes need one.
- **Placement editor.** A move date can be set when the space changes, but there is no screen to
  correct an old placement or a wrong move date afterwards. A future date is refused.
- **Cross-cycle comparison.** Repeat planting creates a new cycle, but comparing cycles in one
  space is not built.
- **Graph view state.** The range, day/night view and sources are remembered per planting
  and space **for the life of the app**, so a weather refresh no longer resets them. They are not
  written to the database, so a restart starts at the defaults again.
- **Extra weather detail.** Hourly weather is stored for the 24-hour and 7-day graph and for the
  day/night averages. There is no separate hourly table view yet; the rain and sunshine graphs
  already draw from the same rows.
- **Weather cleanup.** Rows from an old location stay in the database, and `weather_current` keeps
  one snapshot per sync; there is no cleanup action yet.
- **Weather refresh detail.** An open planting or space page reloads after a successful sync. The
  Settings "Last weather update" row is still built once, so it refreshes when the page or the
  Open-Meteo switch changes, not the second a background call lands. The Weather card is shown for
  outdoor spaces; the same model supplies the sun times behind day and night for every space with
  natural or mixed light.

### App and release

- **CI.** There is no workflow yet; it waits until the repository has a remote. A minimal check can
  come with the GitHub upload and is not part of the first release.
- **Agent skill install.** `assets/agent/SKILL.md` ships with the app, but installing it under
  `~/.agents/skills/omagrow/` is a manual, opt-in step.
- **Media in the CLI.** No command returns a photo's stem and dimensions; the files stay under
  `media/`.
- **MCP.** It comes only if a host needs tool discovery or per-tool approval, and then as a thin
  wrapper over the same functions.

## Future experiments

These are not open roadmap items and not planned for a release.

- **An agent that runs a grow end to end.** Once the app and its validation are solid, the same data
  could in theory let an agent manage a grow by itself: watch readings, decide, and act. Physical
  work such as pruning stays with the user. This is something to test in the future, if time and
  funding allow, not a feature promised for a release. If actuator control is ever tried, it would
  be a separate project with hard limits, an audit log and a stop.

## Open questions

- Should a planting that is merely planned count in the active list, or get its own section? The
  list shows it under Active with a Planned chip; split it out if planned seasons become common.
- How long should finished plantings stay visible before the list offers an archive filter?
- Should a diary entry be editable, or is delete-and-add enough? Editing a repot would need a rule
  for the current pot size; for now a repot edit is not offered.
- Is one photo per diary entry enough, or does the log need a small gallery per entry?
- Should a stage entry be checked against the profile's stage order (no flowering before
  vegetative), or is an out-of-order observation still a valid fact?
- On finish, should the app offer to log a harvest entry before closing the planting?
- Does climate belong to the space only, or can a planting later carry a sensor of its own (soil
  moisture, soil temperature)? The schema leaves room for a planting-level measurement table, but
  the current phase starts with the space.
- Which shared setup fields earn a typed column next (irrigation, fans, CO2), and which stay a
  note until a screen uses them?
- A named space stays in the list when its last planting is deleted. A private space stays hidden
  while its planting exists and is removed with it. Is that the right rule for a garden that is
  resting for a season?
- How long do camera frames stay? One per day is cheap; one per minute is not.
- Does a planting get a frost date of its own, or always use the location's? A greenhouse or a
  balcony can differ from the garden.
- Should a logged stage start belong to a cycle? The store keeps one start per stage, so a second
  flowering in a later perennial year cannot become a stage start; a diary note is the workaround.
- Where does a notification live: on the planting page, the space page, or a small notification
  centre? The rules are set per planting, so it should not become one global feed.
- How long must a wet or dry spell last before a warning is worth saying, and can a warning be
  dismissed or recorded?
- Does the graph's hover readout keep up when the pointer moves quickly? It updates on every motion,
  but it was never exercised with a real pointer during development.
- A manual reading without a time sits at noon; is that the right default, or is a separate "time
  unknown" marker better?

## Deferred decisions

- **Agent session scope.** An agent started for one profile or planting should write there and read
  only what it needs, not the whole database. The CLI has no session concept yet.
- **Conflicting claims and a separate field history.** A `field_claims` store and a separate
  per-field history table are not built; `changes` is the history. A second claim for the same
  field would need its own table before provenance can hold it.
- **Species templates and inheritance.** A profile is per cultivar and self-contained. Copying a
  profile is enough for now. A species template can be added later without breaking the schema.
- **Repeat markers are per stage, not per cycle length.** A stage now says whether it returns each
  year (`repeats`), and the timeline starts each cycle on the anniversary of the first one. A cycle
  that takes two years, or a stage that repeats on a different rhythm, is still not modelled.
- **Per-context requirement sets.** The profile carries traits such as container suitability; the
  planting carries the context. If container and ground needs diverge enough, a planting override
  table can be added later.
- **Hydroponics.** Start with attributes for EC, pH, water temperature and dissolved oxygen.
  Promote to typed fields when a hydroponics screen exists.
- **Pests, diseases and companions.** Start as attributes. A typed risk table comes when the app
  can act on it.
- **Yield and storage.** Attributes first; typed tables when the planner uses them.
- **BBCH.** Optional attribute; no UI requirement.
- **GDD calculation.** Base and target are stored; calculating from weather comes later.
- **Legal status.** Cannabis legality is region/app data, not a plant trait.
- **Localisation.** The UI is English and the frost date order follows `LC_TIME`; units have their
  own preference.
- **Data sources.** RHS, extension services, seed labels and cultivar lists are candidate sources.
  The app ships no world database.
