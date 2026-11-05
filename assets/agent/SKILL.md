---
name: omagrow
description: >
  Read plant profiles from OmaGrow, the local plant and grow journal. Use when the user asks
  about their OmaGrow profiles, their plants' growing data, or wants a plant profile filled in.
---

# OmaGrow

OmaGrow is a local GTK app for plant, garden and grow journals. Data lives in
`~/.local/share/omagrow/omagrow.db` (SQLite). Never open or write that database directly: use the
`omagrow` CLI, which applies the app's validation.

## Commands

```sh
omagrow profile list          # every profile: id, name, species, cultivar
omagrow profile show <id>     # one profile: details, stages, requirements, timing, attributes
omagrow profile create        # new profile; --name, --species, --cultivar
omagrow profile update <id>   # change identity; --name, --species, --cultivar
omagrow profile copy <id>     # copy plant data and provenance; optional identity flags
omagrow attributes list       # the attribute registry: keys, category, type, unit, enum values
omagrow set <id>              # write a JSON document from stdin; --check validates only
omagrow import <file>         # apply a proposal file; --check validates without writing
omagrow agent guide           # this text
```

Every command prints compact JSON on stdout. `list` and `show` are read-only; the profile commands,
`set` and `import` write through the app's validation. There is no delete command: only the user
can delete a profile in the app. Exit codes: `0` success, `1` no database, an old schema or a lookup
problem, `2` wrong usage; messages go to stderr. Reading works while the app is open. If there is
no database yet or the schema is older than the binary, ask the user to open OmaGrow once.

`null` means unknown. Each response carries `api` (the shape version), `app_version` and a
`guide` hint. `profile show` also returns a `vocabulary` object with the closed value sets and
the unit per metric; use those strings and do not invent values.

## Writing

Do not touch the database. Identity is changed with `omagrow profile update`; the `detail` target
in `set` deliberately does not include name, species or cultivar. A copy copies the plant data and
provenance, not photos or history. There is no delete command.

Send a JSON document and validate it before writing:

```sh
omagrow set 5 --check < values.json   # validate and print the change list; writes nothing
omagrow set 5 < values.json           # write it
```

`omagrow import <file>` does the same for a file, including the proposal files the app can review.
The user can revert any change in OmaGrow under Activity, so a wrong value is fixable.

The document mirrors `profile show`:

```json
{"api": 1, "profile": 5, "values": [
  {"target": "detail", "field": "lifecycle", "value": "perennial",
   "source_type": "website", "source_ref": "https://example.org", "confidence": "high"},
  {"target": "stage", "label": "Flowering", "kind": "flowering",
   "duration_days_min": 30, "duration_days_max": 45, "repeats": true, "position": 4},
  {"target": "requirement", "stage": "Flowering", "metric": "air_temp", "day_part": "day",
   "min": 12, "opt_min": 22, "opt_max": 28, "source_type": "book",
   "source_ref": "RHS encyclopedia"},
  {"target": "timing_rule", "kind": "sow_indoor", "anchor": "last_frost",
   "offset_min_days": -70, "offset_max_days": -56, "source_type": "website",
   "source_ref": "https://example.org/sowing"},
  {"target": "attribute", "key": "climate_type", "value": "tropical",
   "source_type": "website", "source_ref": "https://example.org/climate"}
]}
```

- `detail` covers the fields under `profile.details`, not the name, species or cultivar; use
  `omagrow profile update` for those.
- Stages match by label, case-insensitive. A new stage is appended to the end unless you give a
  1-based `position`. Positions are applied after all stages exist, so moving an existing stage
  (for example Ripening to the end) never needs a delete. `repeats: true` marks a perennial stage
  for the yearly cycle, `false` keeps it in the first cycle only; omit the field to keep the
  existing or kind-derived value.
- There is no stage delete target. A stage that already has requirements, timing or attributes
  cannot be deleted in the app either until those are removed. Ask the user if it truly does not
  belong.
- Requirements replace the row for the same stage, metric and day part; timing rules replace the
  rule with the same stage and kind; attributes replace their match. Re-applying is safe.
- Every value except a stage needs a source. Without one the import fails, unless the user turned
  on model knowledge in Settings; such a value is `agent` and always low confidence, even if you
  claim otherwise.
- A value the user confirmed cannot be overwritten; the import stops.
- The file applies as a whole or not at all; an error names the failing value.
- Writing leaves a history row per value; the app's Activity reverts it in one click.
- A file saved in `~/.local/share/omagrow/proposals/` is also offered for review on the profile
  page, for when the user wants to approve before anything is written.

## Reading the data

- `profile.details` holds the core traits: family, genus, lifecycle, life form, foliage,
  determinacy, growth habit, sun exposure, photoperiod, water need, container suitability,
  container volume, days to maturity and notes.
- `profile.stages` is the growing cycle in order. Each stage has a `kind`, a label, an optional
  duration in days and a `repeats` marker for a perennial: `true` returns every year, `false` is
  the first cycle only, `null` follows the stage kind (germination, seedling, establishment and
  juvenile are once-only; the rest returns).
- `profile.requirements` holds ranges per `metric` and `day_part`. A row with `stage_id: null` is
  the profile default; a row with a stage id overrides it for that stage. `day_part: "any"`
  stands in for day and night. A row whose provenance status is `not_applicable` says the plant
  explicitly has no such requirement there; do not propose a value for it.
- `profile.timing_rules` stores relative timing: a `kind` (`sow_indoor`, `sow_outdoor`,
  `transplant`, `harvest`, …), an `anchor` (last frost, first frost, sowing, transplant, stage
  start, day length, GDD, calendar) and an offset in days. Calendar rules use
  `month_start`/`month_end` (1–12); day-length rules use `day_length_hours`; GDD rules use
  `gdd_base_c` and `gdd_target`.
- `profile.attributes` is the long tail. Each value has exactly one of `value_num`, `value_text` or
  `value_bool`: numbers in `value_num`, booleans in `value_bool`, and `text`, `enum` and `json`
  values in `value_text`. Use a key from `omagrow attributes list` when one fits; a custom key is
  a last resort, and an `enum` key only accepts its listed `enum_values`. A definition with
  `applies_to: "stage"` needs a stage; a profile key cannot carry one.

Units are canonical per metric. The `vocabulary.units` object in the JSON is authoritative; this
table is for orientation.

| Metric | Unit |
| --- | --- |
| `air_temp`, `soil_temp` | °C |
| `rh` | % |
| `vpd` | kPa |
| `photoperiod`, `direct_sun_hours` | hours/day |
| `leaf_wetness_hours` | hours |
| `light_dli` | mol/m²/day |
| `light_ppfd` | µmol/m²/s |
| `soil_ph` | pH |
| `soil_ec` | mS/cm |
| `water_mm_week` | mm/week |
| `water_l_week` | L/plant/week |
| `co2` | ppm |
| durations and offsets | days |
| heights, spreads, spacing | cm |

Ranges have three bands: `survival_min`/`survival_max` (outside this the plant dies),
`min`/`max` (growth stops) and `opt_min`/`opt_max` (ideal). The order is
`survival_min ≤ min ≤ opt_min ≤ opt_max ≤ max ≤ survival_max`. Leave a bound out instead of
flipping the order.

Provenance: every row carries a `provenance` object for its whole-row claim (field `*`); the
profile's own `provenance` is keyed by field name. `estimated` needs a source, `confirmed` is the
user's own data and is never overwritten.

## Rules

1. Never invent a value without a source. Name it: a breeder page, a seed packet, an extension
   service or a reference book. Prefer the primary source over a content farm.
2. To start a new variety, copy an existing profile with `omagrow profile copy`, give it the new
   name with `--name`, and adjust species or cultivar in the same command.
3. A wrong value is worse than a gap. Say "unknown" and leave the field out.
4. Never overwrite a value whose provenance status is `confirmed`. That is the user's own data.
5. A value you propose is `estimated` and carries `source_type` plus `source_ref`; confidence is
   `low`, `medium` or `high`.
6. Do not add new enum values, metrics or anchors. If nothing fits, say so and ask.

## What to fill first

1. Ready for advice: lifecycle, the optimum air temperature (day at least), water need, light
   (sun exposure or photoperiod), one stage, days to maturity.
2. Useful: container suitability, soil pH, humidity, stage durations, sowing and harvest timing.
3. Long tail: registry attributes, only those the user asks about.

## Answering

Name the profile and the fields you propose, give each value with its unit and its source, and list
what you could not find. Ask before writing anything the user did not ask for.
