# Weather and local readings

OmaGrow keeps three kinds of air data apart. The graph timeline adds one more table for the hourly
model series. They have different owners, different trust and different cadence, and the interface
shows them in separate sections.

| Kind | Table | What it is |
| --- | --- | --- |
| Weather model | `weather_current`, `weather_daily`, `weather_hours` | Open-Meteo's model value for the location |
| Manual reading | `climate_readings`, source `manual` | A moment the user read from a thermometer |
| Sensor reading | `climate_readings`, source `sensor` | An instrument measurement, when sensors arrive |

A weather value is **not** a measurement in the garden. It is a model for a grid cell, useful for
trends and context, but it never gets the same status as a local reading. The source is always on
the row and the UI labels it as model data.

## Schema 16

### `weather_current`

One row per model moment and location:

```text
weather_current
  id
  latitude, longitude
  observed_on, observed_time          -- provider's local time
  temperature_c, humidity_pct
  apparent_temperature_c
  weather_code                        -- WMO code
  precipitation_mm                    -- the provider's current interval (15 minutes)
  cloud_cover_pct
  wind_speed_kmh, wind_gust_kmh
  is_day
  source, source_ref                  -- weather, Open-Meteo
  fetched_at
```

A unique index on location and observed moment makes a repeated sync harmless.

### `weather_daily`

One row per day and location. The night fields describe the night that **ended on that date**:

```text
weather_daily
  id
  latitude, longitude
  date
  temp_min_c, temp_max_c, temp_mean_c
  humidity_mean_pct, humidity_min_pct, humidity_max_pct
  precipitation_mm, precipitation_hours
  weather_code                        -- dominant daytime condition
  sunshine_hours, daylight_hours
  radiation_sum_mj_m2
  wind_max_kmh, wind_gust_max_kmh
  et0_mm
  sunrise, sunset                     -- local HH:MM
  night_start, night_end              -- local timestamps
  night_temp_avg_c, night_temp_min_c
  night_humidity_avg_pct, night_hours
  source, source_ref
  fetched_at
```

`weather_code` is the most common daytime code, not the most severe code of the day: "partly
cloudy" is a better day summary than "one thunderstorm hour". The daily API's severe code is not
stored yet; warnings can use the hourly data later.

### `weather_hours`

One row per local hour and location. It is the series behind the 24-hour and 7-day graph:

```text
weather_hours
  id
  latitude, longitude
  date, time                          -- local date and HH:MM
  temperature_c, humidity_pct
  precipitation_mm
  weather_code
  cloud_cover_pct
  sunshine_hours
  is_day
  source, source_ref
  fetched_at
```

A unique index on location, date and time makes a repeated sync harmless.

## Synchronising

One API call returns:

- current conditions;
- hourly values for 92 past days and today;
- daily aggregates for 92 past days and today.

The hourly block of today also contains hours that have not happened yet. Those are a forecast,
not a measurement, so **the sync stores only the hours that have passed**; a later sync fills the
rest. The daily row for today is kept for the weather card, but a graph never uses it: for today
the aggregates come from the hourly rows that already passed.

The app calls on start when the last call is at least 15 minutes old, and while it stays open
according to the budget. It makes no calls while Open-Meteo is off; the switch and this budget are
the only weather settings. The 92-day window is what makes the 90-day averages honest. The default
is 12 updates a day (every two hours); the user can change it between 1 and 96. A failed call counts
against the budget, but the next attempt comes sooner instead of waiting the full interval: 15, 30
and 60 minutes, then the normal interval. A successful call clears the failure count. The request
times out after 60 seconds. A successful sync also tells an open planting or space page to reload;
without that, the page kept saying "no data" until the user navigated away and back.

The full 92-day range is fetched every time. That is deliberately simple: the payload is small and
it means a sync always repairs a missed day, a missed night, a missing hour or a gap after the app
was closed. A later optimisation can split current polling from the daily backfill without changing
the model.

The 92-day history comes from Open-Meteo's historical forecast endpoint, not from a station
observation. It is the best available history for a place without a local weather station and it
still returns the current values. The source line says **Open-Meteo model** to keep that distinction
visible.

## Day and night

Daily temperature, humidity, rain, sunshine, radiation and wind come from the provider's daily
aggregates. Two values are computed locally from the hourly series:

- **Dominant daytime condition**: the most frequent WMO code among hours with `is_day = 1`.
- **Night average**: the hours with `is_day = 0` between the previous noon and the current noon.
  The night is assigned to the morning date. Start and end are kept, and a night with fewer than
  two samples is ignored. The oldest backfilled day has no previous evening in the response, so its
  night is left empty rather than stored half-complete.

A night is only written once it has ended. While the app is open during the night, the current
snapshot still updates, but the night average waits for the first sync after sunrise. At 23:00 the
night that ended that morning is complete; the new night is not. A response that cannot complete a night
does not erase a night that was already stored.

## Location changes

Weather rows carry the coordinates they were fetched for. The UI reads the series for the current
location in Settings, so changing the location for a trip or a screenshot does not mix two places.
Old rows stay in the database; there is no automatic cleanup yet. Manual readings never carry a
location.

Replacing an existing location by place or coordinates, and removing it, ask for confirmation
first; setting the first location does not. A successful change asks for a weather sync at once,
within the daily budget, instead of waiting for the interval meant for the old place. The frost
dates stay, but their estimate source is cleared, because it described the old place. Typing in the
coordinate fields saves nothing until the user activates **Use these coordinates**, and a point
close to the saved place keeps its name; a distant point becomes coordinates entered by hand.
Leaving Settings clears the search results, the search query and unsaved coordinate text.

## Interface

- **Space page, outdoor:** a **Weather** card in the rail with the current model value, the last
  seven days and the last completed night. The **Readings** card sits in the same rail, next to the
  Sensors placeholder. Indoor and greenhouse spaces have no Weather card: their air comes from
  local readings and, later, sensors. The stored sun times supply the daylight part for natural
  and mixed light; mixed light also uses its timer (see Graphs).
- **Planting page:** a Weather card in the rail, next to the setup tiles, the diary and the notes. The
  planting owns no readings, so there is no Readings card here; the Climate section draws the
  readings of the space during the planting's placements. The weather card follows the day selected
  in the calendar: today keeps the live view, a past day shows its stored `weather_daily` row and the
  local readings of that day, and a future day says that nothing is stored instead of showing a
  forecast.

## Graphs

One **Climate** section draws temperature, rain, sunshine and humidity. It owns one time range and
one source set; the plots below it are small multiples on that one axis. A wide section lays every
metric with data out as a 2×2 grid; a narrow one shows a single plot with a tab per metric, so each
card keeps the full page width. On a wide page the section spans the full content width. The old
per-card menus, the stage ribbon and the placement label are gone: the timeline above already
carries the stage and the header and setup tiles carry the space.

The header offers **24h**, **7d**, **30d**, **90d** and **All**. Only metrics with data are offered: a
planting or space with no weather and no readings has no Climate section at all, and rain or
sunshine disappears indoors. A metric with data elsewhere stays available when the selected window
is empty; the panel then says why.

- **24h and 7d** use the hourly weather series. The plot has a day/night background and a guide for
  each day part. The 24-hour view of today ends at the last hour that passed and marks **now**, so
  an empty right side reads as "not yet"; a past day is complete. A stored forecast hour is never
  drawn. A day older than the hourly history falls back to its daily row (the mean for air, the
  daily sum for rain and sunshine), never to a forecast.
- **30d, 90d and All** collapse a day into daily values. The view menu decides how:
  - **Mean & range** (default): a continuous min–max band and the daily mean. It answers "what
    happened", without two unexplained lines.
  - **Day & night**: the day average (solid) and the night average (dotted), with both optimum bands
    and a legend.
  - **Day only** and **Night only**: one average and that day part's guide.

  The min–max band is one polygon across the range, not a rectangle per day: at 30 days a per-day
  rectangle reads as a piano of vertical blocks. A tiny legend under the title names the layers that
  are actually drawn.

Sources are separate layers, each with a switch in the section: the **weather model**, a **sensor**
and a **manual** reading. Weather is a line at 24h/7d and a min–max band plus mean above that. A
sensor is a line, broken at gaps and at a placement change; a manual reading stays a point, and a
reading without a time sits at noon. Weather and local readings never share a line. A source row is
shown when more than one source has data, or when a source with data in the window is switched off
so it can be turned back on; a weather-only graph with every relevant source on says **Open-Meteo
model** instead of a switch.

Day and night belong to the space:

- outdoor and natural light follow the sunrise and sunset stored with the weather day;
- LED and HPS follow the light start time and hours on the space;
- mixed light uses the union of daylight and the supplemental window.

LED and HPS light without a start time has no day/night boundary; the graph shows none rather than
a guess. Mixed light without a timer falls back to the sun, because the daylight is still known. The
light settings are copied into a placement, and a change to the shared light reaches the
open placement of a planting that is there now; a closed placement keeps the light it had.

**A planting graph** follows the selected calendar day. A 24-hour window is that day; 7, 30, 90
days and All end on it. Weather only counts during an outdoor placement; readings count during any
placement. A move is a thin divider and a faint tint, not a text label. A range that reaches before
the start or after the end is shaded as outside the grow.

**A space graph** ends today and shows the climate history of the place itself: all local readings
and, for an outdoor space, the weather in the range, also between plantings. It has no profile, so
it has no requirement guides. A rest between plantings is no longer a gap in the history.

The y-scale follows the data and the optimum band. A survival limit is kept when it is within one
extra data span; a limit farther out would flatten the chart, so the plot names it at the top or
bottom edge (with its value) and the tooltip still has it. Rain and sunshine keep 12–15 % headroom,
so a maximum bar never touches the top edge and reads as clipped. Humidity is fixed at 0–100 %, so
a band reads at a glance. The axis uses three to five round labels; the unit sits in the card title,
not on every tick. The plot uses the card's width: the y labels sit inside a measured left margin
and there is only an 8 px pad to the right, so there is no empty column under the old right-hand
labels.

**Guides.** On a planting graph the guides follow the stage of each day. The profile default is
resolved first; a stage row replaces it, an `any` row stands in for day or night, and a
`not_applicable` row removes the guide. The day and night requirements are separate: a stage
without a night row falls back to its `any` row and then the profile; only a `not_applicable` row or
no row at all leaves no night guide. The optimum is a filled band, the survival limits are red
dashed lines, and the tooltip has the values.

**Reading the graph.** Hovering names the day or hour, the weather values, the day and night
averages when the range is collapsed, the local readings and the stage. Above 7 days the pointer
snaps to the day, because a day is one point. The selected calendar day keeps its accent marker in
7d and above; in 24h the whole plot is that day, so only the day and, for today, the now marker
remain.

**Empty ranges.** A panel with no data says why: the last day that has weather, the start of the
data when the window is older than every row, the last failed sync, or that a planting is indoors on
that day. When data exists but every source with data for that graph is switched off, it says so
instead. It never says only "no data".

## Migration and retention

Schema 13 creates the current and daily tables and moves existing `source = 'weather'` rows from
`climate_readings` into `weather_current`. Those old rows had no coordinates, so they take the
current location's coordinates; without a location row they are kept at `0, 0` rather than deleted.
The daily history is filled by the first sync after the upgrade. Schema 14 adds the placement
history used to decide which weather days belong to a planting; schema 16 adds `weather_hours`, the
sun times on the daily row and the light start on a space and its placement. Local readings are
untouched.

Ninety-two days are fetched and kept. A night that was computed once stays when its day becomes the
oldest row; only the oldest day of a fresh backfill has no previous evening and gets no night.
Older daily and hourly rows remain until a cleanup action exists. `weather_current` adds one
snapshot per sync and also has no cleanup yet. The hourly key is local date and `HH:MM`; on the
autumn change the repeated hour overwrites itself, the same limitation the night summary already
had. A sync from before this rule may have left a forecast hour on its own day; the next successful
sync overwrites it with the model's latest value for that past hour, so no manual cleanup is needed.

Switching Open-Meteo off stops new calls; stored weather stays visible on the space and planting
pages.

## Licence

Weather data comes from [Open-Meteo](https://open-meteo.com), CC BY 4.0. Place data comes from
GeoNames. The app needs no API key. The user's "Use Open-Meteo" switch still disables every
request.
