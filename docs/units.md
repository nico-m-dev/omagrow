# Units and the metric/imperial preference

The database stores one unit per value, always metric. The user may see and type imperial; that is a
display and input concern, never a storage or calculation concern.

## The rule

```text
store:      canonical metric, full precision
calculate:  canonical metric only
display:    convert to the user's preference, round for reading
input:      parse the preference, convert to metric, store
agent JSON: canonical metric, always
```

Every conversion happens in one place. No calculation, warning, comparison or agent sees a
converted number. That is what keeps a later calendar, VPD or growing-degree-day sum from mixing
units: they only ever read the stored values.

## Canonical units

| Quantity | Unit | Metric display | Imperial display |
| --- | --- | --- | --- |
| Temperature | °C | °C | °F |
| Temperature difference | °C | °C | °F (`× 9/5`, no offset) |
| Relative humidity | % | % | % |
| Length, height, spacing | cm | cm | in |
| Volume, container | L | L | US gal |
| Amount, feed | ml | ml | ml (no conversion) |
| Mass, harvest | g | g | oz |
| Water per plant | L/plant/week | L/plant/week | US gal/plant/week |
| Water per area | mm/week | mm/week | in/week |
| Light integral | mol/m²/day | mol/m²/day | mol/ft²/day |
| Light intensity | µmol/m²/s | µmol/m²/s | µmol/ft²/s |
| Pressure (VPD) | kPa | kPa | kPa (no common imperial unit) |
| EC | mS/cm | mS/cm | mS/cm |
| Mass per area | kg/m² | kg/m² | oz/ft² |
| Mass per plant | kg/plant | kg/plant | lb/plant |
| Plant density | plants/m² | plants/m² | plants/ft² |
| Duration | days | days | days |

Use the **US liquid gallon** (3.785411784 L), not the imperial gallon. Ambiguity here is worse than
an unusual unit: write "US gal" in the interface when the preference is imperial.

## Traps, and the rule that avoids them

- **Never store what the display shows.** `21.1 °C` rounded from `70 °F` must not overwrite
  `21.111 °C`. Convert only when the user actually edits a field; leave untouched values alone.
- **Differences are not absolute temperatures.** A day–night difference of 6 °C is 10.8 °F, not
  43 °F. Any field that is a delta uses the multiplier only.
- **Ranges compare in canonical units.** `opt_min ≤ opt_max` is checked on stored values, before any
  conversion, so an imperial rounding can never produce an inverted range.
- **One direction of truth for agents.** `omagrow profile show` prints metric and the unit per
  metric; the guide says so. An agent that thinks in inches converts before it proposes, with the
  canonical value in the JSON.
- **Rounding is a display decision.** Show one decimal for °C/°F and inches, two for gallons,
  small masses and per-area values; metric output keeps its full precision as before. Inches keep
  a decimal so a narrow range such as 10–11 cm does not collapse into "4–4 in". The stored value
  keeps full precision.
- **The preference is user-local.** It lives in `settings` like the source policy. It is not part
  of an exported profile and does not change what a proposal contains.

## Where it is

- `src/units.rs` holds `UnitPreference`, `to_display`, `input_to_canonical`, the delta helpers and
  one table for factors, labels and display rounding.
- The preference lives in Settings under **Display**, defaults to metric, and is stored in `settings`.
  It applies to pages you open afterwards; a page that was already open keeps its units until you
  reopen it.
- The read-only profile, the editors, the temperature card, Activity, History, the proposal review,
  the diary amounts, the climate temperature and the space summary use it. The store, the CLI, the
  agent JSON and proposal files stay canonical metric.
- Unknown and custom units pass through unchanged; a custom attribute whose unit happens to read
  "cm" is not converted.
- GDD timing rules stay in °C and °C-days for now. A GDD target depends on the base scale, and
  converting only the base would mix scales; a later GDD slice can convert the pair together. The
  standalone `gdd_base_c` attribute is an ordinary temperature and follows the preference.
- Store and proposal validation errors stay metric, because they guard canonical data. The
  requirement editor pre-checks temperatures so an imperial user sees the display bounds instead.
- Tests cover known conversions, round trips, absolute versus delta temperature, US gallons, the
  display rounding and the preference round trip.

The preference is deliberately **not** derived from the locale or the location. `LC_MEASUREMENT`
follows the installed language and is `en_US.UTF-8` on an English-installed machine in Belgium, so a
locale-driven app would show imperial to a metric user; and the location is where the plant grows,
not where the person reading the screen lives. Units affect data entry, so they get an explicit
choice. Date order is different: it follows `LC_TIME`, because a wrong order is only cosmetic.
