# Plant data model

This document defines the plant data model: what a profile contains, how requirements are expressed,
how time is modelled, and how uncertainty and sources are tracked. The grow layer is in
[plantings.md](plantings.md), the weather model in [weather.md](weather.md); what is not built or
still a question is in [roadmap.md](roadmap.md).

## 1. Purpose

OmaGrow must support a field of maize, an apple tree, a chilli in a greenhouse, a houseplant and a mixed garden. A single flat profile with a handful of fields cannot do
that. The model has to be broad enough for every plant, narrow enough to stay usable, and
typed enough for the app and agents to calculate with.

The guiding idea: **model broad, show narrow**. Data that proves useful is promoted from an open
attribute to a typed field; the schema is allowed to grow in layers.

## 2. The three layers

| Layer | What it is | Example |
| --- | --- | --- |
| **Profile** | A reusable definition of a species or cultivar: what it is and what it needs. | "Bhut Jolokia: day 20–28 °C, night 16–20 °C, full sun, 80–100 days to harvest." |
| **Planting** | One instance in a context, with dates and intentions. | "Three plants in the greenhouse, 20 L pot, sown on 15 March, 12/12 light schedule." |
| **Observation** | A timestamped event with a source. | "1 L water on 2 April", "2 kg harvested on 12 May". |

A **space** (a garden, greenhouse, tent or room) groups plantings that share light and climate.
It is a grouping and an environment, not a fourth kind of plant data; the model is in
[plantings.md](plantings.md).

Rules:

- A profile never contains dates, current values or health status. It contains ranges,
  durations, triggers and traits.
- A planting references a profile and adds context: quantity, container, dates, start kind and user
  intentions. Its space carries the shared light and climate.
- An observation references a planting, never a profile.
- The app will compare observations to requirements once warnings exist. It never stores an
  observation as a requirement.

A plan is derived from the profile, the planting and the user's choices. It is not a fourth
layer with its own plant data.

## 3. Principles

1. **Model broad, UI narrow.** The schema can cover more than the first screen shows.
2. **Requirements are ranges, not single values.** Minimum, maximum and optimum, with day and
   night separated where it matters.
3. **Time is relative, not calendar-bound.** "Six to eight weeks before the last frost" works
   in every hemisphere; "March" does not.
4. **Every value has a status and provenance.** Estimated, confirmed or not applicable, with a
   source and a date. A missing record is unknown.
5. **Unknown is not the same as not applicable.** An annual has no chill-hour requirement; a
   new profile simply has no data yet.
6. **Requirements and observations stay separate.** The profile says what the plant needs;
   the observation says what happened.
7. **Context belongs to the planting.** The profile says a plant can grow in a container and
   how much root space it needs; the planting says which pot it is actually in.
8. **The long tail is typed.** A free-text note such as "pH 6.0–6.8" cannot be compared. An
   attribute with a key, value, unit and stage can.
9. **Controlled vocabulary, with an escape hatch.** Agents and imports use registered keys;
   custom keys are allowed but marked as custom.
10. **No silent defaults.** The app does not calculate with a guessed value for something
    nobody knows.
11. **Metric internally.** Store °C, centimetres, litres and days; convert for display later.
12. **No bundled world database.** Profiles are authored by the user or an agent, with sources.

## 4. Dimensions

Every value in the model can carry one or more of these dimensions:

| Dimension | Values | Why |
| --- | --- | --- |
| **Stage** | germination, seedling, juvenile, vegetative, flowering, fruiting, ripening, harvest, dormancy, senescence, establishment, mature | A seedling and a flowering plant have different needs. |
| **Day part** | day, night, any | Temperature and humidity minima differ between day and night; pH does not. |
| **Lifecycle** | annual, biennial, perennial, ephemeral | Determines which fields are applicable. |
| **Context** | ground, container, greenhouse, indoor, hydroponics | Lives on the planting, with a few profile traits such as container suitability. |
| **Time anchor** | sowing, transplant, last frost, first frost, stage start, day length, growing degree days, calendar | Makes timing portable across locations. |
| **Confidence** | estimated, confirmed, not applicable, or absent (unknown) | Prevents guesses from looking like facts. |

## 5. Stages

### 5.1 Decision

Each profile has an **ordered list of stages**. Every stage has:

- a **semantic kind** from a controlled vocabulary, so the app can reason about it;
- a **free label**, so the user can call it what they call it;
- a position in the sequence;
- an optional duration range;
- an optional trigger;
- requirements and notes.

This is deliberately not a fixed global stage list. Maize, an apple tree and a houseplant do not share one sequence. It is also not free text only: the app needs to know which
stage is flowering, which is harvest and which is dormancy.

Multiple stages of the same kind are allowed, for example two flowering flushes or a repeated
harvest period. The stage list describes one growing cycle. For a perennial, each stage says
whether it returns every year: a stage marked to repeat is part of the yearly cycle, while a
once-only stage (germination, seedling, establishment, juvenile) happens at the start of the
plant's life.
The marker is stored per stage, so a houseplant can repeat only its mature state and a fruit tree
can repeat dormancy through ripening. See [plantings.md](plantings.md) for how the timeline uses it.

A new profile starts empty. Lifecycle starter templates are not built; the roadmap keeps that
idea.

### 5.2 Stage kinds

| Kind | Meaning | Typical examples |
| --- | --- | --- |
| `germination` | From sowing to the first true leaves. | Seed tray, direct sowing. |
| `establishment` | A cutting, transplant or young perennial settling in. | Rooted cutting, transplanted seedling, young tree. |
| `seedling` | Young plant after germination, before vigorous vegetative growth. | Tomato seedling, young chilli. |
| `juvenile` | Young perennial that cannot flower or fruit yet. | Apple sapling, young berry bush. |
| `vegetative` | Roots, stems and leaves grow; no reproductive growth yet. | Maize before tasselling, cannabis in veg. |
| `flowering` | Flower formation, bloom and pollination. | Chilli flowering, apple blossom. |
| `fruiting` | Fruit or seed development after successful pollination. | Tomato fruit set, maize ears. |
| `ripening` | Fruit or seed matures; harvest approaches. | Tomato turning red, cannabis ripening. |
| `harvest` | The period in which the usable part is taken. | Cutting lettuce, picking beans. |
| `dormancy` | Growth pauses; the plant survives unfavourable conditions. | Deciduous tree in winter, houseplant in winter. |
| `senescence` | The plant dies back after its cycle. | Annual at the end of the season. |
| `mature` | A long-lived plant with no distinct juvenile or harvest stage in the current cycle. When it is the last stage and the only yearly stage, it continues until the end instead of restarting each year. | Many houseplants, established ornamental perennials. |

The vocabulary is semantic, not botanical. It should cover the app's calculations, not every
phenological nuance. A stage can carry a free label such as "tasselling" or "stretch" while its
kind remains `flowering` or `vegetative`.

### 5.3 Stage fields

| Field | Storage | Notes |
| --- | --- | --- |
| `position` | core | Order within the profile's cycle. |
| `kind` | core | Controlled vocabulary above. |
| `label` | core | User-facing name; may differ from the kind. |
| `duration_days_min` / `duration_days_max` | core | Optional range; some stages are trigger-driven. |
| `repeats` | core | Whether the stage returns every year. Empty means the kind-derived rule: `germination`, `seedling`, `establishment` and `juvenile` happen once, the rest returns. |
| `trigger` | timing | Optional rule that starts the stage; see section 8. |
| `notes` | core | Free text. |
| `bbch_range` | attribute | Optional, agent-facing; profile-level today. A stage-scoped key still needs the store API. |

### 5.4 BBCH codes

BBCH is a standardised 0–99 scale for phenological growth stages, originally developed by the
German Biologische Bundesanstalt, Bundessortenamt and Chemische Industrie. Principal stages run
from 0 (germination) through leaf development, stem elongation, inflorescence emergence,
flowering, fruit development and ripening to 9 (senescence). "BBCH 65", for example, means full
flowering.

BBCH is useful for agents because agricultural sources, weather models and extension services
often use it. It is too technical for the normal UI. Store it as an optional attribute and let an
agent map a stage to a BBCH range when a source provides one; a stage-scoped key is still
store-only, so the regular UI writes a profile-level value. The app does not require BBCH codes to
function.

## 6. Data inventory

The **Storage** column says where a field belongs:

- **core**: a typed column on `profiles`, because the app calculates with it;
- **requirement**: a row in `profile_requirements`, because it is a range per stage and day part;
- **timing**: a row in `profile_timing_rules`, because it is a relative rule;
- **attribute**: a registered key in `attribute_definitions`, with its value in
  `profile_attributes`. An attribute holds one number, text, boolean or enum value of its
  registered type; it does not store a range.

Fields that were once designed but are neither a column nor a registered attribute are listed in
[roadmap.md](roadmap.md); this inventory describes what exists.

### 6.1 Identity and classification

| Field | Storage | Notes |
| --- | --- | --- |
| `name` | core | Common name; required. |
| `species` | core | Latin binomial. |
| `cultivar` | core | Cultivar or variety name. |
| `synonyms` | attribute | Other names, trade names, old names. |
| `family` | core | Botanical family; useful for rotation. |
| `genus` | core | Optional; can be derived from species later. |
| `uses` | attribute | Culinary, medicinal, ornamental, cut flower, dye, fibre, cover crop. |
| `lifecycle` | core | Annual, biennial, perennial, ephemeral. |
| `life_form` | core | Herbaceous, woody, vine, grass, succulent, bulb, tuber. |
| `foliage` | core | Evergreen, deciduous, semi-evergreen. |
| `determinacy` | core | Determinate, indeterminate, not applicable. |
| `growth_habit` | core | Upright, bushy, climbing, creeping, rosette, spreading. |
| `mature_height_cm` | attribute | One number in cm. |
| `mature_spread_cm` | attribute | One number in cm. |
| `lifespan_years` | attribute | One number in years; especially for perennials. |
| `years_to_first_harvest` | attribute | One number in years; especially for trees and shrubs. |
| `allergens` | attribute | Pollen, sap, spines, etc. |

### 6.2 Lifecycle and timing

| Field | Storage | Notes |
| --- | --- | --- |
| Stage list | core | Ordered stages; see section 5. |
| `duration_days` per stage | core | Range. |
| `days_to_maturity` | core | Summary range; derived from stages where possible. |
| `flowering_period` | timing | Seasonal or day-length driven; kind `flowering`. |
| `fruiting_period` | timing | Seasonal; kind `fruiting`. |
| `dormancy_period` | timing | Seasonal; kind `dormancy`. |
| `gdd_base_c` | attribute | Growing degree day base temperature. |
| `gdd_target` | timing | Target per stage or for maturity. |
| `vernalization_temp_max_c` | attribute | For biennials and some perennials. |
| `vernalization_days` | attribute | Duration at vernalizing temperature. |
| `alternate_bearing` | attribute | Some fruit trees crop heavily one year and lightly the next. |
| `bbch_range` | attribute | Optional, agent-facing; profile-level today. |

### 6.3 Climate and hardiness

| Field | Storage | Notes |
| --- | --- | --- |
| Frost tolerance per stage | requirement | `air_temp` with `survival_min` per stage. |
| `heat_tolerance_max_c` | attribute | Temperature above which damage occurs. |
| `climate_type` | attribute | Tropical, subtropical, temperate, mediterranean, arid, continental. |
| `chill_hours` | attribute | Hours below a chilling threshold. |
| `drought_tolerance` | attribute | Low, medium, high. |
| `waterlogging_tolerance` | attribute | Low, medium, high. |
| `salt_tolerance` | attribute | Low, medium, high. |
| `wind_tolerance` | attribute | Low, medium, high. |
| `rainfall_need_mm` | attribute | Annual or seasonal amount in mm; outdoor planning. |
| `shelter_need` | attribute | Windbreak, shade cloth, frost cover. |
| `native_range` | attribute | Geographic origin; context for climate. |

### 6.4 Light and photoperiod

| Field | Storage | Notes |
| --- | --- | --- |
| `sun_exposure` | core | Full sun, partial sun, partial shade, full shade. |
| `direct_sun_hours` | requirement | Range; can vary by stage. |
| `light_dli` | requirement | Daily light integral, mol/m²/day; indoor. |
| `light_ppfd` | requirement | µmol/m²/s; indoor, instantaneous. |
| `photoperiod_type` | core | Short-day, long-day, day-neutral, autoflowering. |
| `photoperiod_obligate` | core | Whether the photoperiod is required or only influential. |
| `critical_day_length_hours` | core | Threshold for short-day and long-day plants. |
| `photoperiod` | requirement | Target hours per day, per stage. |
| `shade_tolerance` | attribute | Per stage; qualitative. |
| `artificial_light_suitable` | attribute | Whether the plant performs indoors under lamps. |
| `light_spectrum_notes` | attribute | Blue/red ratio, UV, etc. |
| `light_hours` on a space | core | The actual schedule is grow data, not profile data; see [plantings.md](plantings.md). |

### 6.5 Temperature, humidity and VPD

All of these are **requirements**, expressed per stage and, where relevant, per day part.
Section 7 defines the range semantics.

| Metric | Unit | Day parts | Notes |
| --- | --- | --- | --- |
| `air_temp` | °C | day, night | The core temperature metric. |
| `soil_temp` | °C | any | Germination and rooting. |
| `rh` | % | day, night | Relative humidity. |
| `vpd` | kPa | day, night | Vapour pressure deficit; a target, derived from temperature and RH. |
| `co2` | ppm | any | Indoor enrichment; no dedicated card yet. |
| `leaf_wetness_hours` | hours | any | Disease pressure; registered, no dedicated card yet. |

Additional attributes:

| Field | Storage | Notes |
| --- | --- | --- |
| `dif_c` | attribute | Day–night temperature difference; indoor. |
| `thermoperiod_notes` | attribute | Qualitative notes. |
| `frost_protection` | attribute | Methods and thresholds. |

### 6.6 Water, substrate and nutrition

| Field | Storage | Notes |
| --- | --- | --- |
| `water_need` | core | Low, medium, high; a quick summary. |
| `water_mm_week` | requirement | Outdoor, ground. |
| `water_l_week` | requirement | Containers; per plant. |
| `drought_tolerance` | attribute | Low, medium, high. |
| `waterlogging_tolerance` | attribute | Low, medium, high. |
| `water_quality_sensitivity` | attribute | Chlorine, hardness, salinity. |
| `root_depth_cm` | attribute | One number in cm; bed and container planning. |
| `soil_texture` | attribute | Sand, loam, silt, clay; a list or a texture class. |
| `soil_drainage` | attribute | Poor, moderate, free. |
| `soil_organic_matter_pct` | attribute | One number in percent. |
| `soil_ph` | requirement | Min, optimum, max. |
| `soil_ec` | requirement | Min, optimum, max; containers and hydroponics. |
| `fertility_need` | attribute | Low, medium, high. |
| `feed_frequency_days` | timing | A `feed` rule, often relative to a stage start, with `repeat_days`. |
| `container_suitable` | core | Boolean. |
| `min_container_volume_l` | core | Range. |
| `root_restriction_tolerance` | attribute | Low, medium, high. |
| `substrate_recipe` | attribute | Peat, coco, compost, perlite ratios; free text or structured. |
| `mulch_recommended` | attribute | Boolean plus notes. |
| `salt_tolerance` | attribute | Low, medium, high. |
| `mycorrhiza` | attribute | Whether inoculation is useful. |

Soil is deliberately split by context. Natural soil, container substrate and hydroponics have
different parameters; one "ideal soil composition" string would hide that.

### 6.7 Propagation and planting

| Field | Storage | Notes |
| --- | --- | --- |
| `propagation_methods` | attribute | Seed, cutting, division, layering, tuber, bulb, grafting. |
| Method details | attribute | Season, difficulty, temperature, humidity, days, success rate. |
| `seed_depth_cm` | attribute | One number in cm. |
| `germination_temp` | requirement | `soil_temp` during germination. |
| Germination duration | core | The `germination` stage's duration range. |
| `germination_light` | attribute | Light, dark or either. |
| `stratification_days` | attribute | Cold treatment. |
| `stratification_temp_c` | attribute | Cold treatment temperature. |
| `scarification` | attribute | Mechanical or thermal. |
| `soak_hours` | attribute | Pre-sowing soak. |
| `seed_viability_years` | attribute | Storage life. |
| `transplant_age_weeks` | attribute | One number in weeks. |
| `transplant_tolerance` | attribute | Low, medium, high; root disturbance. |
| `hardening_off_days` | attribute | One number in days. |
| `planting_depth_cm` | attribute | One number in cm. |
| `spacing_in_row_cm` | attribute | One number in cm. |
| `spacing_between_rows_cm` | attribute | One number in cm. |
| `plants_per_m2` | attribute | One number; derived from spacing where possible. |
| `sowing_window` | timing | Relative to last frost or a season; kind `sow_indoor` or `sow_outdoor`. |
| `planting_window` | timing | Relative to last frost or a season; kind `transplant`. |
| `graft_rootstock` | attribute | For grafted trees. |
| `graft_compatibility` | attribute | Notes. |
| `seed_saving_isolation_distance_m` | attribute | Optional. |

### 6.8 Care

| Field | Storage | Notes |
| --- | --- | --- |
| Pruning window | timing | Kind `prune`. |
| `pruning_wood_age` | attribute | Old wood, new wood, both. |
| `pruning_method` | attribute | Heading, thinning, topping, renewal, deadheading. |
| `pruning_intensity` | attribute | Light, moderate, hard. |
| `pruning_purpose` | attribute | Shape, airflow, yield, disease removal. |
| `pruning_aftercare` | attribute | Notes. |
| Training methods | attribute | Stake, trellis, LST, SCROG, espalier. |
| `thinning` | timing | Fruit thinning, seedling thinning; kind `thinning`. |
| `deadheading` | timing | Whether and when; kind `deadheading`. |
| `pollinator_partner` | attribute | Required cross-pollinator. |
| `flowering_group` | attribute | For fruit trees. |
| `support_type` | attribute | None, stake, cage, trellis; a `support` rule can carry the timing. |
| `winter_protection` | attribute, timing | Mulch, fleece, move indoors; kind `winter_protect`. |
| `repot_interval_months` | attribute, timing | Houseplants and containers; kind `repot`. |
| `pot_bound_sensitivity` | attribute | Low, medium, high. |
| `rotation_group` | attribute | For vegetable gardens. |

### 6.9 Harvest and use

| Field | Storage | Notes |
| --- | --- | --- |
| `harvest_parts` | attribute | Leaf, fruit, root, seed, flower, stem, tuber, bulb. |
| `harvest_indicators` | attribute | Colour, firmness, size, days, trichomes, scent. |
| `harvest_window` | timing | Relative to sowing, transplant or flowering. |
| `harvest_method` | attribute | Cut, pick, dig, pull. |
| `yield_per_plant` | attribute | One number in kg/plant. |
| `yield_per_m2` | attribute | One number in kg/m². |
| `succession_interval_days` | timing | Repeated sowings; kind `succession`. |
| `storage_temp_c` | attribute | One number in °C. |
| `storage_rh_pct` | attribute | One number in percent. |
| `shelf_life_days` | attribute | One number in days. |
| `processing` | attribute | Drying, curing, freezing, canning. |
| `flower_color` | attribute | Ornamentals. |
| `flower_scent` | attribute | Ornamentals. |
| `bloom_period` | timing | Ornamentals; kind `bloom`. |
| `cut_flower_suitable` | attribute | Boolean. |
| `landscape_use` | attribute | Border, ground cover, hedge, specimen. |

### 6.10 Risks and relations

| Field | Storage | Notes |
| --- | --- | --- |
| `pest_susceptibility` | attribute | Name, susceptibility, conditions, prevention. |
| `disease_susceptibility` | attribute | Name, susceptibility, conditions, prevention. |
| `companions_good` | attribute | Species plus reason. |
| `companions_bad` | attribute | Species plus reason. |
| `allelopathy` | attribute | Whether the plant suppresses neighbours. |
| `sensitive_to` | attribute | Foot traffic, pollution, salt, wind. |

### 6.11 Provenance

Provenance is not a section in the UI. It is a property of every value. Section 10 defines the
model.

## 7. Requirements

### 7.1 Why rows

A requirement is stored as a row, not as a set of columns on the profile:

```text
profile_requirements
  id
  profile_id
  stage_id          -- NULL means: profile-level default
  metric            -- controlled vocabulary, e.g. air_temp
  day_part          -- day, night, any
  min_value
  max_value
  opt_min
  opt_max
  survival_min
  survival_max
```

Provenance lives in `field_provenance` under the row's id. This directly answers the day/night
question: temperature and humidity get separate day and night rows, each with its own range.
Adding a metric later is a row, not a migration of the whole schema.

### 7.2 Metrics and units

The metric registry defines the canonical unit. Inputs in other units are converted at the
edge; stored values are always canonical.

| Metric | Unit | Day parts | Notes |
| --- | --- | --- | --- |
| `air_temp` | °C | day, night | Air temperature. |
| `soil_temp` | °C | any | Germination, rooting. |
| `rh` | % | day, night | Relative humidity. |
| `vpd` | kPa | day, night | Target; derived from temperature and RH. |
| `photoperiod` | hours/day | any | Target light period. |
| `direct_sun_hours` | hours/day | any | Direct sunlight; outdoor. |
| `light_dli` | mol/m²/day | any | Daily light integral. |
| `light_ppfd` | µmol/m²/s | any | Instantaneous light. |
| `soil_ph` | pH | any | 0–14. |
| `soil_ec` | mS/cm | any | Containers and hydroponics. |
| `water_mm_week` | mm/week | any | Ground. |
| `water_l_week` | L/plant/week | any | Containers. |
| `co2` | ppm | any | Indoor enrichment. |
| `leaf_wetness_hours` | hours | any | Disease pressure. |

### 7.3 Range semantics

Three bands are available. All are optional; the app uses the most specific one it has.

| Band | Meaning | Example |
| --- | --- | --- |
| `survival_min` / `survival_max` | Outside this, the plant is damaged or dies. | Tomato: below 0 °C is fatal. |
| `min` / `max` | Outside this, growth stops or is seriously impaired. | Tomato: below 10 °C growth stalls. |
| `opt_min` / `opt_max` | The ideal range for growth and production. | Tomato: 20–26 °C by day. |

The tomato example shows why one min/max is not enough: it survives at 5 °C, grows poorly below
10 °C, thrives at 20–26 °C, and fails to set fruit above 30 °C. The app can warn differently
for each band once warnings exist.

Validation: `survival_min ≤ min ≤ opt_min ≤ opt_max ≤ max ≤ survival_max`. A missing bound is
allowed; a contradictory order is not.

### 7.4 Inheritance

Resolution order for a given profile, stage, metric and day part:

1. the stage-specific row, if present;
2. the profile-level row (`stage_id IS NULL`), if present;
3. otherwise, unknown.

A stage row with status `not_applicable` overrides a profile default and explicitly says
"this does not apply here". Resolution reports that as its own answer instead of falling back to
the profile default or pretending the value is unknown. Within one scope a specific day part
wins over `any`, and a `not_applicable` row at the scope that wins stops the search there.

The store implements the resolution. The planting graph uses it for the stage of each day, so a
stage row changes the day and night guides for that stage and a day without a stage falls back to
the profile default. The profile page still shows the rows as stored; using the resolution there is
listed in [roadmap.md](roadmap.md).

### 7.5 Status

A requirement row can be:

- **estimated**: an agent or import proposed it, with a source;
- **confirmed**: the user checked it;
- **not applicable**: the plant or stage does not have this requirement.

Unknown is represented by the absence of a row. The app does not store empty rows just to say
"we don't know". A `not_applicable` row still carries bounds for now; only its provenance says
the requirement does not apply, so the provenance write path has to set that status deliberately.

Completeness treats `not_applicable` as an answer: it satisfies the advice gate and the progress
bar, the summary says how many values are not applicable, and an agent is told not to fill them.
A stage with a timing rule counts as dated even when it has no duration, because the rule supplies
its start.

A narrower row can make a confirmed default stop applying. An agent may do that only with an
explicit `override_confirmed`, and never over a `not_applicable` value; the profile page marks such
an estimated stage row as overriding the confirmed default.

## 8. Timing

### 8.1 Why relative time

A calendar month is wrong outside its climate and hemisphere. The model stores a **rule** with
an anchor and an offset, and the app resolves it against the planting's location and dates.

The frost anchors are the exception that needs a datum: the last spring frost and the first autumn
frost belong to the location. The user enters them, or Settings estimates them from thirty years of
ERA5-Land daily minima; an estimate keeps its source line and stops being an estimate when the user
edits a date. The dates are stored as `MM-DD`; the app reads and shows them in the order of the
`LC_TIME` date locale (day-first unless that locale is from a month-first country), which never
changes the stored value. Without them a rule such as
"sow indoors 70–56 days before the last frost" cannot become a calendar date; the rule itself is
still valid.

The frost estimate downloads the daily minimum temperature for 1991–2020 from the archive API
(ERA5-Land, a grid of about 9 km) and takes the 9-in-10 date: per year the last freezing day before
the warm season and the first one after it, then the 90th and 10th percentile. It is air
temperature at 2 m, so ground frost can arrive earlier, and a valley or a city centre differs from
the grid cell. A year with fewer than 150 days of data in a season is skipped. The year is split at
1 July, so spring is the first half in the north and the second half in the south; a season without
frost keeps the date that was already there. The result is a suggestion with a source line: editing
a date or changing the location removes that line, because it no longer describes these dates.

```text
profile_timing_rules
  id
  profile_id
  stage_id          -- optional; the stage this rule belongs to
  kind              -- sow_indoor, sow_outdoor, transplant, harvest, prune, feed, ...
  anchor            -- last_frost, first_frost, sowing, transplant, stage_start, day_length, gdd, calendar
  offset_min_days
  offset_max_days
  month_start       -- calendar fallback
  month_end
  day_length_hours  -- day-length trigger
  gdd_base_c        -- GDD trigger
  gdd_target
  repeat_days       -- recurring care or succession
  notes
```

### 8.2 Anchors

| Anchor | Meaning | Example |
| --- | --- | --- |
| `last_frost` | The local average last spring frost. | Sow indoors 6–8 weeks before last frost. |
| `first_frost` | The local average first autumn frost. | Harvest before first frost. |
| `sowing` | The planting's sowing date. | Germination 10–14 days after sowing. |
| `transplant` | The planting's transplant date. | Harvest 80–100 days after transplant. |
| `stage_start` | The start of a named stage. | Feed every 14 days during vegetative growth. |
| `day_length` | A day-length threshold. | Flowering at 12 hours of light. |
| `gdd` | Accumulated growing degree days. | Maturity at 1,200 GDD, base 10 °C. |
| `calendar` | A month or season fallback. | Repot in spring. |

### 8.3 Examples

| Rule | Anchor | Offset |
| --- | --- | --- |
| Sow indoors | `last_frost` | −56 to −42 days |
| Transplant outdoors | `last_frost` | 0 to +14 days |
| Harvest | `transplant` | +80 to +100 days |
| Prune | `stage_start` (dormancy) or `last_frost` | Stage- or season-relative |
| Flowering | `day_length` | 12 hours |
| Feed | `stage_start` | Every 14 days during vegetative growth |
| Succession sowing | `sowing` | Every 21 days during the sowing window |

Calendar rules are a fallback for plants without a clear anchor, such as "repot in spring" for a
houseplant. They are less portable and should not replace an anchor when one exists.

### 8.4 GDD

Growing degree days are optional. Store the base temperature and a target per stage or for
maturity. The app can calculate accumulated GDD from weather data later. Until then, GDD is
stored data, not a required input.

## 9. Attributes

### 9.1 Why a registry

The long tail changes constantly: pH, EC, VPD, cold hours, sowing depth, trichome colour,
mowing height, seeding rate, graft rootstock. If every agent invents its own key, the data
cannot be compared. The registry gives each known attribute a stable key, label, type and unit.

```text
attribute_definitions
  key               -- e.g. climate_type, stratification_days, yield_per_plant
  label
  description
  value_type        -- number, text, boolean, enum, json
  unit              -- optional
  category          -- About, Climate, Soil & nutrition, Propagation, Care & training,
                    -- Harvest & storage, Problems, Garden & use, Other
  applies_to        -- profile, stage, planting, observation
  allowed_stages    -- optional
  allowed_lifecycles
  enum_values
  is_custom
  validation...
```

`profile_attributes` stores values:

```text
profile_attributes
  id
  profile_id
  stage_id          -- optional
  key
  value_num
  value_text
  value_bool
```

Exactly one value column is used, according to the definition's type. Provenance lives in
`field_provenance`.

### 9.2 Custom keys

A custom key is allowed when no registered key fits. It is marked `is_custom = true` and shown
as custom in the UI. Agents must use a registered key when one exists. A frequent custom key can
later be promoted to the core.

### 9.3 Examples of long-tail attributes

- `climate_type`, `chill_hours`, `gdd_base_c`
- `soil_texture`, `soil_drainage`, `substrate_recipe`
- `stratification_days`, `scarification`, `soak_hours`, `seed_viability_years`
- `graft_rootstock`, `graft_compatibility`
- `training_method`, `support_type`, `pruning_aftercare`
- `yield_per_plant`, `yield_per_m2`, `storage_temp_c`, `shelf_life_days`
- `flower_color`, `flower_scent`, `cut_flower_suitable`
- `companions_good`, `companions_bad`, `allelopathy`
- `pest_susceptibility`, `disease_susceptibility`
- `mowing_height`, `seeding_rate`, `n_fixation`
- `trichome_harvest`, `flush_days`

Many of these will become typed tables when a screen needs them. Until then, they are data
with a stable key.

## 10. Provenance

### 10.1 Status

Every value can be:

| Status | Meaning |
| --- | --- |
| absent | Nobody has filled it in; shown as unverified. |
| `estimated` | An agent, import or source proposed it; not checked by the user. |
| `confirmed` | The user checked it. |
| `not_applicable` | The plant or stage does not have this requirement. |

### 10.2 Source

Each value records:

- `source_type`: user, agent, import, label, book, website;
- `source_ref`: URL, book title, seed packet, extension service;
- `retrieved_at`: date;
- `confidence`: low, medium, high;
- `notes`.

Provenance holds one status and source per value today. Conflicting claims are not stored side by
side yet; that needs a separate claims table, which is deferred in [roadmap.md](roadmap.md). A
confirmed value is never overwritten silently.

### 10.3 Implementation

A generic `field_provenance` table keyed by entity, entity id and field keeps provenance in one
place:

```text
field_provenance
  entity_type       -- profile, stage, requirement, timing_rule, attribute
  entity_id
  field             -- column or attribute key; '*' for a whole row
  status
  source_type
  source_ref
  retrieved_at
  confidence
  notes
  updated_at
```

Provenance is per value, not per profile. Every profile value write also leaves a row in the change
ledger, except a provenance status change or a stage reorder; [history.md](history.md) describes the
row, the revert and the exceptions.

## 11. Storage schema

### 11.1 Tables

| Table | Holds |
| --- | --- |
| `profiles` | Identity and scalar traits: name, species, cultivar, family, genus, lifecycle, life form, foliage, determinacy, growth habit, sun exposure, photoperiod type and obligate flag, critical day length, water need, container suitability and minimum volume, days to maturity, notes. |
| `profile_stages` | Ordered stages: position, kind, label, duration range, notes and the `repeats` marker. |
| `profile_requirements` | Ranges per profile or stage, metric and day part: air temperature, soil temperature, RH, VPD, photoperiod, DLI, PPFD, pH, EC, water, CO2, leaf wetness. |
| `profile_timing_rules` | Relative rules for sowing, transplanting, harvest, pruning, feeding and other care. |
| `attribute_definitions` | The controlled vocabulary for the long tail. |
| `profile_attributes` | Values for the long tail, optionally per stage. |
| `field_provenance` | Status and source per value. |

Migration 4 implements the identity fields plus the scalar traits above. Later migrations add
`media` (5), `settings` (6), `changes` (7), attribute categories (8), place name, region, country
and timezone on `locations` (9), `spaces` and `plantings` (10), `observations` for the diary (11),
local `climate_readings` on a space (12), `weather_current` plus `weather_daily` (13),
`planting_placements` (14), the stage `repeats` marker (15), `weather_hours` plus the sun times and
light start (16), and `avatar_icons` (17). The grow tables are described in
[plantings.md](plantings.md), the weather model in [weather.md](weather.md).

### 11.2 Why so few tables

The typed core covers what the app calculates with now: stages, ranges and timing. Everything
else starts as a registered attribute. When a group earns its own screen and validation, it gets
its own typed table. This is the "promote from attribute to core" path.

The weather model is `weather_current`, `weather_daily` and `weather_hours` (schema 16). Tables that were once
sketched but are not built — conflicting claims, a separate field history, per-planting overrides,
species templates — are in [roadmap.md](roadmap.md).

## 12. Validation

All writes, whether from the UI, an import or an agent, go through the same validation.

- Ranges: `survival_min ≤ min ≤ opt_min ≤ opt_max ≤ max ≤ survival_max`. Missing bounds are
  allowed; contradictions are not.
- A requirement needs at least one bound; an empty row must not shadow a profile default.
- Humidity: 0–100%. pH: 0–14%. Hours per day: 0–24.
- EC, DLI, PPFD, water, CO2, days, spacing, volume: non-negative.
- Temperature: reject values outside −60–80 °C; that is a sanity check, not a hard botanical
  limit.
- Stage positions: unique and ordered.
- Timing: `offset_min_days ≤ offset_max_days`; a rule needs a known kind and anchor.
- Attributes: value type must match the definition; a custom key is marked custom.
- Profile names: required and non-empty, also outside the UI.
- Provenance: an estimated value needs a source. Confirmed and not-applicable values are
  protected from agent writes. Every profile value write leaves a history row, with the exceptions
  listed in [history.md](history.md) (provenance status changes and stage reordering).
- Units: the store, the CLI and agent JSON are canonical metric. The UI converts at input and
  display; see [units.md](units.md).

## 13. UI shape

The model is broad; the profile page stays narrow.

- **Quick facts** and a **temperature card** with day and night bands, both fed by the stored
  rows and attributes.
- **Growing cycle**: stages in order, with reorder arrows, then requirements and timing. A
  stage's requirements stay attached to that stage.
- **Plant traits** and **attributes grouped by category**. A custom key picks one of the fixed
  categories.
- Every detail row, requirement, stage, attribute and at-a-glance tile has a status chip:
  unverified, estimated, confirmed or not applicable. Clicking it confirms, unconfirms or marks
  the value not applicable. A value edited in the app becomes confirmed with source `user`.
  Identity — name, species and cultivar — has history but no chip.
- The separate editors cover the profile form, the cycle pages and the attribute page. Cycle and
  attribute rows save immediately; the profile form saves on Save and asks before discarding.
- Activity shows the newest changes with the value; History groups them by day and reverts a row.
  A proposal file in `proposals/` appears as **Proposals → Review**.
- A warning, when warnings exist, compares an observation to the requirement bands and says which
  band was exceeded: survival, growth or optimum.

## 14. Use-case stress tests

| Use case | What the model uses |
| --- | --- |
| Maize field | Annual lifecycle, GDD, wind pollination, row spacing, nitrogen need, frost sensitivity, harvest window. |
| Apple tree | Perennial, juvenile stage, chill hours, pollinator partner, pruning on old wood, years to first harvest, hardiness. |
| Chilli in a greenhouse | Day-neutral photoperiod, temperature and RH per stage, EC/pH, container volume, pruning. |
| Houseplant | Low DLI, container volume, RH, repotting interval, toxicity, no frost. |
| Cannabis indoors | Photoperiod or autoflower, light schedule, DLI, VPD, EC/pH, training, trichome harvest. |
| Mixed garden | Companions, rotation group, succession sowing, shade tolerance, yield. |

Cannabis is a useful stress test because it combines photoperiod, light intensity, VPD,
container chemistry and training. It must not drive the model: the same fields have to work for
an apple tree and a houseplant. Some stress-test fields are attributes today, not typed columns;
that is the intended shape.

## 15. Glossary

| Term | Meaning |
| --- | --- |
| **BBCH** | A standardised 0–99 scale for plant growth stages. |
| **Chill hours** | Hours below a chilling threshold that some perennials need to break dormancy and flower. |
| **Cultivar** | A cultivated variety, for example 'Bhut Jolokia'. |
| **DLI** | Daily light integral: the total amount of light a plant receives per day, in mol/m²/day. |
| **EC** | Electrical conductivity, a proxy for nutrient concentration, in mS/cm. |
| **GDD** | Growing degree days: accumulated heat above a base temperature, used to predict development. |
| **PPFD** | Photosynthetic photon flux density: instantaneous light intensity, in µmol/m²/s. |
| **RH** | Relative humidity, in percent. |
| **VPD** | Vapour pressure deficit: the drying power of the air, in kPa; derived from temperature and RH. |
| **Vernalisation** | Cold treatment that some biennials and perennials need before flowering. |
| **Photoperiod** | The daily light period; some plants flower only when it crosses a threshold. |
| **Determinate** | Growth stops at a certain point, often after flowering; the opposite is indeterminate. |
