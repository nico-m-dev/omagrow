# Plantings and spaces

This document describes the grow layer as it exists. What is not built or still a question is
collected in [roadmap.md](roadmap.md).

This document defines how OmaGrow models what is actually growing. It sits next to
[plant-data.md](plant-data.md): the profile says what a plant needs, the planting says what you
did, and the space says where and in which shared environment.

## 1. Three layers, one grouping

| Layer | What it is | Example |
| --- | --- | --- |
| **Profile** | A reusable definition of a variety: needs, stages and timing. | "Bhut Jolokia: day 20–28 °C, 80–100 days to harvest." |
| **Space** | A physical place whose environment is shared. | "Grow tent", "Greenhouse 1", "Back garden". |
| **Planting** | One variety, one group, one cycle, in one space. | "5 Northern Light plants in the grow tent, started 4 Feb." |
| **Observation** | A timestamped event or measurement of the planting. | "2 L water on 12 May". |

The space is a grouping and an environment, not a fourth layer of plant data. A planting always
belongs to exactly one space. A space without a name is a private space for a single planting:
the user never has to create a place before creating a planting.

## 2. The sharing rule

**If two plantings share the air, they share the space.**

A space owns what is physically common:

- light: natural, LED, HPS or mixed, the photoperiod in hours and, for a powered light, the local
  time it comes on;
- local readings: air temperature and relative humidity, by hand or sensor;
- the weather model for the location: one sky for every space, and the sun times that natural or
  mixed light follows;
- sensors later;
- the layout of the space later.

An avatar is identity, not shared setup: a space has one, and every planting has its own. A planting
never borrows the profile's photo, because one variety can stand in several spaces with a different
picture in each. The rule and the storage are in [media.md](media.md).

A planting owns what belongs to that group of plants:

- the profile, quantity, container type, volume and substrate;
- the start date and start kind;
- the diary: water, feed, prune, repot, pest, photo, harvest;
- stage milestones;
- the end date and outcome.

This is not an arbitrary split. Two plantings in one greenhouse stand under the same lamp and
breathe the same air; two pots in that greenhouse are still separate pots. If two groups need
different light or climate, they are two spaces — even when they are in the same room.

The user never has to visit a space page first. The planting form creates a space inline, or
reuses an existing one, and shows the shared setup there. Editing the shared setup from a
planting changes it for every planting in that space; the form says so.

**The space page** (Spaces in the sidebar) is the overview the other way around: the named spaces
with their shared type and light, and how many plantings are active. The space list shows the space's
own avatar: a photo, a chosen place illustration or the fallback for its kind (garden bed,
greenhouse or indoor shelf). On the space page the avatar sits
above the shared setup, not in the top bar. A space page shows that setup, its plantings and their
state, and the data its graphs draw from. The readings card and its page are in section 3.4; the
weather card and graph semantics are in [weather.md](weather.md), and the wide-window layout in
[development.md](development.md). Edit opens the same form a planting opens, and a save there
changes it for everything inside. Its type is fixed once the space has plantings, placements or readings; name,
notes and light can still change, and a light change reaches the open placement of a planting that is
there now. A private space has no name, is not a place of its own and is not listed. It is kept
hidden when it has measurement history, so a move does not delete the old readings. A planting opened
from a space stays in that context, so Back returns to the space rather than to the planting list.

## 3. Data model

### 3.1 Spaces and plantings (schema 10)

```text
spaces
  id            INTEGER PRIMARY KEY
  name          TEXT              -- NULL: a planting with its own private space
  kind          TEXT              -- outdoor, greenhouse, indoor
  light_type    TEXT              -- natural, led, hps, mixed; optional
  light_hours   REAL              -- photoperiod; optional
  light_start   INTEGER           -- minutes after local midnight; optional
  notes         TEXT
  created_at, updated_at

plantings
  id                  INTEGER PRIMARY KEY
  profile_id          INTEGER NOT NULL REFERENCES profiles(id) ON DELETE RESTRICT
  space_id            INTEGER NOT NULL REFERENCES spaces(id) ON DELETE RESTRICT
  name                TEXT       -- NULL: use the profile name
  quantity            INTEGER NOT NULL DEFAULT 1
  container           TEXT       -- ground, pot, bed, hydroponics
  container_volume_l  REAL       -- per pot; optional
  substrate           TEXT
  started_on          TEXT       -- ISO date
  start_kind          TEXT       -- sown_indoor, sown_outdoor, transplanted, planted
  ended_on            TEXT       -- ISO date; NULL while growing or planned
  outcome             TEXT       -- harvested, removed, died, failed
  notes               TEXT
  created_at, updated_at
```

A unique index on `spaces(name COLLATE NOCASE)` where the name is not null stops two spaces with
the same name. A planting with a private space has `space_id` set to a row whose name is null;
those rows are not listed as places.

`ON DELETE RESTRICT` protects history. A profile with plantings cannot be deleted. A named space
cannot be deleted yet. A private space stays with its planting and is removed when that planting is deleted. After a
move it stays hidden but keeps its readings, so the move does not delete the old data.
Deleting a planting cascades to its diary rows and its private spaces, including any readings
left there; the app removes their photo files too.

### 3.2 Placements (schema 14)

A planting can move between spaces: a chilli starts indoors, moves to a greenhouse, then goes
outside. The space stays the owner of its measurements; the planting gets a **placement history**
so those measurements can be selected for the right period.

```text
planting_placements
  id
  planting_id
  space_id
  started_on
  ended_on            -- NULL = the current placement
  space_kind          -- snapshot: outdoor, greenhouse, indoor
  light_type, light_hours, light_start
  created_at
```

- One open placement per planting; a unique partial index enforces it.
- A move closes the current placement on the day before the move and opens the next one on the
  move date. Both placement ends are inclusive, so a finished planting keeps its last day. A
  reading is matched by its space as well as its date: a reading taken in the old space on the
  move day stays with that space, is absent from the planting's active-period climate section on
  that day, and remains visible on the space's Readings page.
- A move on the same day as the placement started updates that placement instead of creating a
  zero-length period.
- Finishing closes the open placement; reopening opens the last one again.
- Deleting a planting removes its placements and its private spaces, including their readings. While
  the planting exists, a private space with readings or placements stays hidden, so a move does not
  cascade-delete its measurements.

The snapshot of `space_kind` is what lets a planting show weather only while it was outdoors. A
space's type is fixed once it has plantings, placements or readings; name, notes and light can
still change.
That keeps old measurements from being reinterpreted as another environment. If a different type
is needed, create a new space and move the planting.

### 3.3 Diary (schema 11)

```text
observations
  id               INTEGER PRIMARY KEY
  planting_id      INTEGER NOT NULL REFERENCES plantings(id) ON DELETE CASCADE
  kind             TEXT       -- water, feed, prune, repot, pest, disease, note, photo,
                              -- harvest, stage, support, other
  occurred_on      TEXT       -- ISO date
  occurred_time    TEXT       -- local HH:MM; optional
  note             TEXT
  amount           REAL       -- canonical: L, ml or g, depending on the kind
  unit             TEXT       -- L, ml or g
  stage_id         INTEGER REFERENCES profile_stages(id) ON DELETE SET NULL
  previous_amount  REAL       -- for a repot: the old pot volume
  previous_unit    TEXT
  created_at, updated_at
```

### 3.4 Local readings (schema 12)

```text
climate_readings
  id              INTEGER PRIMARY KEY
  space_id        INTEGER NOT NULL REFERENCES spaces(id) ON DELETE CASCADE
  measured_on     TEXT       -- local ISO date
  measured_time   TEXT       -- local HH:MM; optional
  temperature_c   REAL       -- canonical °C; at least one of temperature or humidity
  humidity_pct    REAL       -- percent
  source          TEXT       -- manual; sensor later
  source_ref      TEXT       -- sensor name; optional
  created_at
```

A local reading is a fact of the space: a hand-read thermometer now, a sensor later. It is never
edited; delete it and add it again. Weather-model data does not live here; it has its own tables
(section 3.5) so the two never look like the same kind of measurement.

### 3.5 Weather model (schema 16)

Weather belongs to the location, not to one space: every space shares the same sky, and an outdoor
space draws its weather metrics from it. It lives in `weather_current`, `weather_daily` and
`weather_hours`, separately from local readings, and is labelled as model data. The schema, the sync
and the graph rules are in [weather.md](weather.md); this layer only selects the rows for the
current location and the planting's outdoor placements.

### 3.6 State is derived, not stored

| State | Rule |
| --- | --- |
| **Planned** | `started_on` is after today. |
| **Growing** | `started_on` is today or earlier and `ended_on` is null. |
| **Finished** | `ended_on` is set. |

There is no status column that can drift out of step with the dates. `outcome` is required when
`ended_on` is set and must be empty otherwise. `ended_on` may not be before `started_on`.

### 3.7 Validation

- The profile exists.
- Quantity is at least 1.
- `container` is one of the four known values; a volume is non-negative and only allowed for
  `pot`.
- `started_on` parses as a date; `start_kind` is one of the four known values.
- The space exists; a new named space is unique.
- A private space (a space with no name) holds at most one planting. A space with more than one
  planting cannot lose its name.
- Light hours are between 0 and 24 when set. A light start time is between 00:00 and 23:59, needs
  light hours, and belongs to LED, HPS or mixed light.
- No planting can be created without a profile or a space.
- A space with plantings, placements or readings keeps its type. A new environment is a new space.
- A move date cannot be before the planting start, before the current placement started, or in the
  future for a planting that has already started.
- A finished planting must be reopened before it can move. Only one placement per planting is open.
- A start date cannot change in the same save as a space move, or after the first placement ends.
  Within the first placement, a wrong start date can be corrected.
- A reading belongs to a planting when its date falls inside that planting's placement and its
  space matches the placement. Placement ends are inclusive; a move closes the old placement on
  the day before and opens the new one on the move date.

Diary entries add their own checks:

- The kind is one of the known kinds; a stage entry needs a stage that belongs to the planting's
  profile.
- The date may not be before the planting started. A later entry is allowed, so a harvest note
  after the finish date still works.
- The amount is non-negative and uses the kind's canonical unit: L for water and repot, ml for
  feed, g for harvest. Kinds without an amount reject one.
- A repot is only allowed for a pot, and updates the current pot volume in the same transaction.
- A stage start belongs to the cycle nearest to its date; one start per occurrence, oldest first.

Local readings add:

- The space exists; at least one of temperature and humidity is present.
- The date is not after today: a reading is a measurement, not a plan.
- Temperature is canonical °C and humidity is 0–100%.
- The source is `manual` or `sensor`; the app writes `manual` by hand, and the sensor source is
  accepted by the store without a UI yet.

Weather rows are validated by the weather layer; see [weather.md](weather.md).

All writes go through the store, so the UI and a later agent CLI share the same checks.

### 3.8 History scope

The change ledger is profile-only; planting setup and the diary are not in it. The scope is in
[history.md](history.md) and the gap is in [roadmap.md](roadmap.md).

## 4. The create flow

The planting is the entry point. A space is never a prerequisite.

1. **Choose the plant.** A searchable list of profiles with a photo or plant mark. The only
   required choice. With no profiles, a link switches to the Profiles section.
2. **Set it up.** One form, everything else optional:
   - name (defaults to the profile name);
   - quantity (1);
   - container (ground, pot, raised bed, hydroponics) and pot volume;
   - substrate;
   - start date (today) and start kind (planted);
   - space: on its own, an existing space, or a new one;
   - for a new or private space: type and light.
3. **Create.** The app lands on the planting page.

A second planting in the same space only needs the plant, the quantity and the start date: the
shared light is already there and shown as a summary.

## 5. The planting page

The overview is one scroll; the diary and weather cards are pushed pages or cards from the
overview.

- **Overview**: the planting's own avatar, status, day count, space and quantity; the setup facts;
  the timeline and calendar; the climate section; a diary card for the selected day; a Weather card
  for an outdoor space; the profile's growing cycle as a plain list when a stage has no duration;
  notes. The avatar is a photo, a chosen crop illustration or the neutral sprout; it does not
  inherit the profile's photo. Click it for **Show photo**, **Add/Change photo**, **Remove photo**
  and **Choose illustration**; the picker searches the crops for a profile or planting and shows
  the places for a space. Edit, Finish and Repeat live
  here. The calendar selection drives the diary, weather, timeline and climate section
  (section 6.3). The wide-window layout is in [development.md](development.md).
- **Diary**: the full log, newest first, grouped by day, with filters for care, problems, notes,
  stages and harvest. Each entry has a kind, date, optional local time, an optional note, an
  optional amount in the user's units, and at most one photo. A stage start is logged from the Grow
  timeline; the entry it creates shows up here.
- **Weather** (4b): an outdoor space or an outdoors planting shows the Open-Meteo card. The model,
  sync, sun times and card contents are in [weather.md](weather.md).
- **Readings** (4b): the local measurements of the space; see section 3.4. A planting does not own
  them.
- **Climate**: one section with a shared range and source set. The header, ranges, day/night views,
  source switches and graph semantics are in [weather.md](weather.md). A planting section follows
  the selected calendar day and shades time outside its active period; a space section is the
  climate history of the place itself, including a rest between plantings. Readings keep their own
  card in the space rail.
- **Moving**: editing a planting and choosing another space shows a **Moved on** date, today by
  default. The store closes the previous placement the day before and opens the new one on that
  date. A finished planting must be reopened before it can move. A start-date typo can be corrected
  within the first placement, but not across a move.

The diary kinds are water, feed, prune, repot, pest, disease, note, photo, harvest, stage, support
and other. The log form offers all but `stage`, which the grow timeline creates. Amounts are
canonical metric (`L` for water and repot, `ml` for feed, `g` for harvest)
and convert only at input and display. A repot updates the planting's current pot volume in the
same transaction and keeps the previous volume on the entry, so the log shows `3 L → 11 L`.
Repots on the same day follow their local time; one without a time sorts before the timed entries
of that day. Deleting a repot does not roll the current pot size back; the entry's note and amount
are history.

A **stage** entry records that a stage started, and it is logged from the grow timeline (**Stage
started…** under the card), not from the diary form: it corrects the plan, so the action belongs with
the blocks it moves. A perennial shows the same stage in every cycle, so the schedule matches the
logged date to the **nearest occurrence** of that stage: the previous stage of that cycle ends there,
the logged stage starts there, and the rest of that cycle follows from it. An earlier or later cycle
keeps its own dates. One start per occurrence, oldest first; a second date that lands on an already
claimed occurrence is ignored. The diary entry stays in the log even when the stage is not on the
timeline (skipped or not placed), so it can be read and deleted there; a logged start without a
duration is a point on the timeline.

**Readings** belong to the space. A reading holds a local date and an optional local time, air
temperature in canonical °C and relative humidity in percent; at least one of the two is required.
It also holds its source (`manual` today, `sensor` later) and an optional reference to the sensor.
The space page's card shows the last seven days (lowest and highest temperature, average humidity)
and adds a reading; the Readings page lists every reading and deletes one. A planting does not own
readings: its Climate section draws the ones of the space during its placements. A reading is never
edited: delete it and add it again.

**Weather is model data for the location, not a reading.** An outdoor space, or a planting while it
is outdoors, shows the Open-Meteo values for the place in Settings. It lives in `weather_current`,
`weather_daily` and `weather_hours`, never in `climate_readings`, and the interface labels it as
model data. The sync, budget, sun times and day/night rules are in [weather.md](weather.md).
Indoor and greenhouse spaces take their air from local readings instead and, later, sensors.

Finishing a planting asks for a date and an outcome. The planting stays in the list under
"Finished"; it is never removed automatically. **Repeat planting** on a finished planting creates a
new cycle with the same profile, quantity, container, substrate, space and light, starting today
and without the old diary. A named space is reused; a private space is copied, because it belongs
to one planting. Comparing cycles in one space is not built yet ([roadmap.md](roadmap.md)).

## 6. Timeline and calendar

Both are derived; no dates are stored. The schedule is rebuilt from the profile, the planting's
real start and the frost dates every time the page opens.

### 6.1 Stage windows

- A sowing, or a planted tuber, bulb or tree, starts at the first stage the profile describes. A
  transplant skips `germination` and `seedling` and starts at `establishment`, `vegetative`,
  `juvenile` or `mature`.
- Each stage gets an uncertain window from the cumulative minimum and maximum durations, plus a
  likely core from the midpoint. The timeline draws the window faintly and the core solidly.
- A stage without a duration stops the placement and is listed under "Not dated yet"; the page
  falls back to the plain cycle list and never invents a date. A stage whose start was logged
  without a duration is a point on the timeline, with a note that only the start is known.
- A stage start logged from the timeline replaces the estimate for that stage **in its own cycle**:
  it claims the occurrence nearest to the logged date, its start becomes that date, the previous
  stage of that cycle ends there, and later stages of that cycle follow from the new end. One start
  per occurrence, oldest first; a second date on a claimed occurrence is ignored, so it never moves
  the next cycle. A logged start that falls before the previous stage is reported and keeps the
  profile order on the timeline. The timeline marks each logged start with a small dot.
- **A year around the selected day, zoomable.** The timeline shows 366 days, six months either side of
  the day selected in the calendar, so the marker sits in the middle and a stage is wide enough to
  read. The two buttons in the card's header zoom in to six months and then to three months, and back
  out again; the selected day stays the centre, and the chosen level is kept while the page is open.
  Under the card, **Stage started…** opens a small form for the selected day: the stage, the date and
  an optional note, with no amount and no photo. It creates the same diary entry the log used to.
  The ticks fall on the first of the month (a quarter apart on a year), and stages that cross an edge
  are clipped. It never scrolls: the calendar is how you move the window. The tooltip lists the stages
  of the visible range.
- **A perennial repeats its cycle.** A stage marked **Repeats each year** returns after the first
  cycle; the default for each stage kind is in [plant-data.md](plant-data.md). The profile editor
  shows the choice per stage, and the timeline repeats exactly the marked stages in profile order. Each new cycle starts on the anniversary of the first yearly stage's first start (the same
  month and day), not immediately after the previous cycle, so a cycle shorter than a year leaves
  an honest gap instead of drifting out of season. It repeats until the planting's end, or until
  today plus one full cycle for a growing plant, capped at twenty cycles. Every repeated stage
  needs a duration, and a stage that the first cycle could not place is not repeated; a logged stage
  start still belongs to the first cycle, and a timing rule anchored to a stage uses the first time
  that stage appears. A frost or calendar care rule (prune, harvest, feed, …) repeats for every year
  the timeline covers, up to the planting's end; a sowing, transplant, succession or thinning rule
  stays once. The repeated windows are estimates. A **single mature stage** is the exception: when
  it is the last stage and the only yearly stage, it is a continuing adult state, so it runs from
  its first start until the planting ends, or one year past today for a growing plant, and it is not
  restarted each year. A finished planting keeps only the stages whose likely start is before its
  end, and a stage that was cut short ends on the end date.

### 6.2 Milestones

The calendar and the "next" line combine:

- the planting's start (a fact);
- every stage start (estimated, or logged from the timeline);
- every timing rule in the profile;
- diary entries that are not stage starts (water, feed, note, photo, harvest, ...).

Anchors resolve like this:

| Anchor | Base |
| --- | --- |
| `last_frost`, `first_frost` | The frost dates in Settings, applied to the start year; a first-frost or calendar window before the start rolls to the next year. |
| `sowing` | The planting start; for a transplant, the midpoint of a sowing rule before it. |
| `transplant` | The planting start when it was transplanted or planted; otherwise the midpoint of the transplant rule. |
| `stage_start` | The likely start of the rule's stage. |
| `calendar` | The month window in the start year, or the next year when it lies before the start. |
| `day_length`, `gdd` | Reported as unknown until day length and weather data exist. |

A rule with a range becomes a window (a harvest period). A rule that repeats the planting's own
start event (the same kind, with a window that contains the start date) is not added twice; a
later transplant, for example, still appears. A missing anchor is reported under "Not
dated yet", not guessed. The schedule's "next" is the first milestone that has not fully passed,
including a diary entry dated today or later; a window that is open today says "Now: … until …".

### 6.3 The page

- **Timeline**: the stage windows of the visible year, the selected day as an accent marker in the
  middle, a muted today marker when it falls in the same year, the ended marker of a finished
  planting, the year's dates and the next milestone. The day line in the header describes the
  selected date — its day number and likely stage — and falls back to today's status when the
  selected day is today. When two markers sit too close for both labels, the selected one keeps its
  label.
- **Calendar**: a month grid with a dot per milestone and diary entry: stage starts, sow,
  transplant, harvest, care (water, feed, prune, repot, support), problems (pest, disease) and
  notes/photos. Up to three dots per day; the tooltip and day detail list the rest. The diary's
  stage entry is not repeated next to the schedule's stage-start milestone. Previous/next/today and
  a selected-day line with the events and the likely stage are included.
- The calendar opens on today for a growing planting, on the start date for a planned one and on
  the end date for a finished one.
- **Only days of the grow are selectable.** A day before the planting's start or after its end is
  disabled and keeps its place in the grid, so a rest around the grow is visible but cannot be
  chosen. A growing planting has no end yet, so its future days stay available (with the "no weather
  stored yet" weather card). The **Today** button is disabled when today itself lies outside the
  grow, and the "not today" banner only appears while today is a day of the grow. A finished
  planting therefore opens on its last day, with the Today button disabled and no banner, which is
  the honest view of a cycle that is over.

**The selected day drives the page.** A click does not rebuild anything: the calendar moves its
highlight and detail line, and the diary card, weather card, timeline and climate section each
update in place, so the scroll position stays where it was. The page listens to the calendar
directly; the calendar only asks the page for a new day.

- **Banner**: while the selected day is not today, a banner sits in the second top bar, under the
  header: "Viewing 12 October 2026 — not today" with **Back to today**. It stays visible while the
  page scrolls. Back to today clears it, returns the calendar to the current month and selects
  today.
- **Calendar**: the selected day gets a strong accent fill, a border and a bold accent number;
  today keeps a light neutral box and an accent number, so both stay readable when they are the same
  day. The line under the grid names the selected day, its milestones and its likely stage.
- **Diary card**: the entries of the selected day, not the newest three. An empty day says so; the
  full diary page still exists.
- **Weather card**: today keeps the live view; a past day shows that day's stored `weather_daily`
  row and the local readings of that day; a future day says "No weather stored for a future day."
  There is no forecast.
- **Climate section**: the selected day anchors the 24h, 7d, 30d, 90d and All windows. A click updates
  the section in place; the range, view and sources survive a weather refresh while the app runs.
  In 24h the plot is the selected day, so it has no separate day marker; a past day is
  complete and today ends at **now**. In 7d and above the selected day keeps its accent marker. A
  day outside the data simply has no line.
- **Persistence**: the selection survives a visit to the diary and other pages pushed from the
  planting page. It resets when the planting page is left.
- **Month browsing**: the arrows move the grid only. The selected day, and with it the page, stays
  where it was; clicking a day in another month selects that day.

A logged stage start turns the estimate into fact from that point on. A global calendar across all
plantings is listed in [roadmap.md](roadmap.md).
