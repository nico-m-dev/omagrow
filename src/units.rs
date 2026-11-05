//! Canonical metric storage, the user's display preference, and the one conversion table.
//!
//! Values are stored and calculated in metric units. Only input and display convert. pH, EC, RH,
//! VPD, days and unknown units stay as they are: there is no sensible imperial counterpart, and a
//! custom attribute unit must never be guessed at.

use crate::store::{self, Store};

const KEY: &str = "units";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UnitPreference {
    #[default]
    Metric,
    Imperial,
}

impl UnitPreference {
    pub fn load(store: &Store) -> Self {
        match store.setting(KEY).ok().flatten().as_deref() {
            Some("imperial") => Self::Imperial,
            _ => Self::Metric,
        }
    }

    pub fn save(self, store: &Store) -> store::Result<()> {
        store.set_setting(
            KEY,
            match self {
                Self::Metric => "metric",
                Self::Imperial => "imperial",
            },
        )
    }

    pub fn is_imperial(self) -> bool {
        self == Self::Imperial
    }

    /// A canonical value as the user sees it.
    pub fn to_display(self, unit: &str, value: f64) -> f64 {
        if !self.is_imperial() {
            return value;
        }

        match unit {
            "°C" => value * 9.0 / 5.0 + 32.0,
            _ => match factor(unit) {
                Some(factor) => value * factor,
                None => value,
            },
        }
    }

    /// A typed value back to canonical metric.
    pub fn input_to_canonical(self, unit: &str, value: f64) -> f64 {
        if !self.is_imperial() {
            return value;
        }

        match unit {
            "°C" => (value - 32.0) * 5.0 / 9.0,
            _ => match factor(unit) {
                Some(factor) => value / factor,
                None => value,
            },
        }
    }

    /// A temperature *difference*: `6 °C` becomes `10.8 °F`, not `42.8 °F`.
    pub fn delta_to_display(self, unit: &str, value: f64) -> f64 {
        if self.is_imperial() && unit == "°C" {
            value * 9.0 / 5.0
        } else {
            self.to_display(unit, value)
        }
    }

    pub fn delta_input_to_canonical(self, unit: &str, value: f64) -> f64 {
        if self.is_imperial() && unit == "°C" {
            value * 5.0 / 9.0
        } else {
            self.input_to_canonical(unit, value)
        }
    }

    /// The unit label to print next to a converted value.
    pub fn label(self, unit: &str) -> String {
        if !self.is_imperial() {
            return unit.to_string();
        }

        match unit {
            "°C" => "°F",
            "cm" | "mm" => "in",
            "m" => "ft",
            "L" => "US gal",
            "L/plant/week" => "US gal/plant/week",
            "kg" => "lb",
            "g" => "oz",
            "kg/plant" => "lb/plant",
            "kg/m²" | "g/m²" => "oz/ft²",
            "mol/m²/day" => "mol/ft²/day",
            "µmol/m²/s" => "µmol/ft²/s",
            "mm/week" => "in/week",
            "plants/m²" => "plants/ft²",
            other => other,
        }
        .to_string()
    }

    /// A canonical value, converted and rounded for reading. The unit label is separate.
    pub fn number(self, unit: &str, value: f64) -> String {
        let value = self.to_display(unit, value);
        self.round(unit, value)
    }

    /// The same for a temperature difference.
    pub fn delta_number(self, unit: &str, value: f64) -> String {
        let value = self.delta_to_display(unit, value);
        self.round(unit, value)
    }

    fn round(self, unit: &str, value: f64) -> String {
        if !self.is_imperial() || !is_converted(unit) {
            return metric_number(value);
        }

        let decimals = match unit {
            "°C" => 1,
            "cm" | "mm" | "m" => 1,
            "L" | "L/plant/week" => 2,
            "kg" | "kg/plant" => 1,
            "g" => 2,
            "kg/m²" | "g/m²" => 2,
            "mol/m²/day" => 2,
            "µmol/m²/s" => 2,
            "mm/week" => 2,
            "plants/m²" => 2,
            _ => 2,
        };

        trim(format!("{value:.decimals$}"))
    }

    /// Convert, round and label in one call for read-only text.
    pub fn text(self, unit: &str, value: f64) -> String {
        match unit {
            "" => self.number(unit, value),
            "%" => format!("{}%", self.number(unit, value)),
            _ => format!("{} {}", self.number(unit, value), self.label(unit)),
        }
    }

    /// A range or a one-sided bound, with the unit only once.
    pub fn range(self, min: Option<f64>, max: Option<f64>, unit: &str) -> Option<String> {
        let body = match (min, max) {
            (Some(min), Some(max)) if min == max => self.number(unit, min),
            (Some(min), Some(max)) => {
                format!("{}–{}", self.number(unit, min), self.number(unit, max))
            }
            (Some(min), None) => format!("from {}", self.number(unit, min)),
            (None, Some(max)) => format!("up to {}", self.number(unit, max)),
            (None, None) => return None,
        };

        Some(match unit {
            "" => body,
            "%" => format!("{body}%"),
            _ => format!("{body} {}", self.label(unit)),
        })
    }
}

/// The canonical unit of a profile detail field that carries one.
pub fn detail_unit(field: &str) -> &'static str {
    match field {
        "min_container_volume_l_min" | "min_container_volume_l_max" => "L",
        _ => "",
    }
}

/// Whether this unit changes under the imperial preference. A plain count, day, percentage or pH
/// must keep its full precision.
fn is_converted(unit: &str) -> bool {
    matches!(
        unit,
        "°C" | "cm"
            | "mm"
            | "m"
            | "L"
            | "L/plant/week"
            | "kg"
            | "g"
            | "kg/plant"
            | "kg/m²"
            | "g/m²"
            | "mol/m²/day"
            | "µmol/m²/s"
            | "mm/week"
            | "plants/m²"
    )
}

/// Multipliers from a canonical metric unit to its imperial display unit.
fn factor(unit: &str) -> Option<f64> {
    Some(match unit {
        "cm" => 1.0 / 2.54,
        "mm" => 1.0 / 25.4,
        "m" => 3.280_839_895,
        "L" | "L/plant/week" => 1.0 / 3.785_411_784,
        "kg" | "kg/plant" => 2.204_622_622,
        "g" => 0.035_273_962,
        // kg/m² and g/m² both become oz/ft².
        "kg/m²" => 3.277_058,
        "g/m²" => 0.003_277_058,
        "mol/m²/day" | "µmol/m²/s" | "plants/m²" => 0.092_903_04,
        "mm/week" => 1.0 / 25.4,
        _ => return None,
    })
}

fn metric_number(value: f64) -> String {
    if value == 0.0 {
        "0".to_string()
    } else {
        value.to_string()
    }
}

fn trim(mut text: String) -> String {
    if text.contains('.') {
        while text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
    }

    if text == "-0" { "0".to_string() } else { text }
}

#[cfg(test)]
mod tests {
    use super::UnitPreference;
    use crate::store::Store;

    #[test]
    fn metric_leaves_everything_alone() {
        let metric = UnitPreference::Metric;
        assert_eq!(metric.to_display("°C", 20.0), 20.0);
        assert_eq!(metric.input_to_canonical("cm", 2.54), 2.54);
        assert_eq!(metric.label("L"), "L");
        assert_eq!(metric.text("°C", 20.0), "20 °C");
        assert_eq!(metric.text("%", 50.0), "50%");
        assert_eq!(metric.number("days", 12.0), "12");
    }

    #[test]
    fn temperature_is_absolute_and_delta_is_not() {
        let imperial = UnitPreference::Imperial;
        assert_eq!(imperial.to_display("°C", 20.0), 68.0);
        assert_eq!(imperial.input_to_canonical("°C", 68.0), 20.0);
        assert_eq!(imperial.delta_to_display("°C", 6.0), 10.8);
        assert_eq!(imperial.delta_input_to_canonical("°C", 10.8), 6.0);
        assert_eq!(imperial.text("°C", 20.0), "68 °F");
    }

    #[test]
    fn converts_length_volume_and_mass() {
        let imperial = UnitPreference::Imperial;

        assert!((imperial.to_display("cm", 2.54) - 1.0).abs() < 1e-9);
        assert_eq!(imperial.text("cm", 10.0), "3.9 in");
        assert_eq!(imperial.text("m", 1.0), "3.3 ft");
        assert_eq!(imperial.label("mm"), "in");
        assert_eq!(imperial.text("kg", 100.0), "220.5 lb");
        assert_eq!(imperial.text("g", 100.0), "3.53 oz");
        assert_eq!(imperial.text("kg/plant", 1.0), "2.2 lb/plant");
        assert_eq!(imperial.label("L/plant/week"), "US gal/plant/week");
        assert_eq!(
            imperial.text("L/plant/week", 3.785_411_784),
            "1 US gal/plant/week"
        );
    }

    #[test]
    fn converts_per_area_units() {
        let imperial = UnitPreference::Imperial;

        assert_eq!(imperial.number("mol/m²/day", 20.0), "1.86");
        assert_eq!(imperial.number("µmol/m²/s", 500.0), "46.45");
        assert_eq!(imperial.number("mm/week", 50.0), "1.97");
        assert_eq!(imperial.text("plants/m²", 10.0), "0.93 plants/ft²");
        assert_eq!(imperial.label("kg/m²"), "oz/ft²");
    }

    #[test]
    fn formats_ranges_with_the_label_once() {
        assert_eq!(
            UnitPreference::Metric
                .range(Some(10.0), Some(20.0), "°C")
                .as_deref(),
            Some("10–20 °C")
        );
        assert_eq!(
            UnitPreference::Imperial
                .range(Some(10.0), Some(20.0), "L")
                .as_deref(),
            Some("2.64–5.28 US gal")
        );
        assert_eq!(
            UnitPreference::Imperial
                .range(None, Some(50.0), "%")
                .as_deref(),
            Some("up to 50%")
        );
    }

    #[test]
    fn unknown_units_and_numbers_stay_put() {
        let imperial = UnitPreference::Imperial;
        assert_eq!(imperial.to_display("pH", 6.5), 6.5);
        assert_eq!(imperial.label("pH"), "pH");
        assert_eq!(imperial.text("days", 12.0), "12 days");
        assert_eq!(imperial.text("days", 12.345), "12.345 days");
        assert_eq!(imperial.text("", 1.2345), "1.2345");
        assert_eq!(imperial.to_display("%", 50.0), 50.0);
    }

    #[test]
    fn round_trips_stay_close() {
        let imperial = UnitPreference::Imperial;

        for unit in [
            "°C",
            "cm",
            "mm",
            "m",
            "L",
            "kg",
            "g",
            "kg/m²",
            "g/m²",
            "mol/m²/day",
            "mm/week",
        ] {
            for value in [1.0, 3.785_411_784, 12.5, 100.0] {
                let shown = imperial.to_display(unit, value);
                let back = imperial.input_to_canonical(unit, shown);
                assert!(
                    (back - value).abs() < 1e-9,
                    "{unit}: {value} -> {shown} -> {back}"
                );
            }
        }
    }

    #[test]
    fn the_setting_survives_a_roundtrip() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(UnitPreference::load(&store), UnitPreference::Metric);

        UnitPreference::Imperial.save(&store).unwrap();
        assert_eq!(UnitPreference::load(&store), UnitPreference::Imperial);

        UnitPreference::Metric.save(&store).unwrap();
        assert_eq!(UnitPreference::load(&store), UnitPreference::Metric);
    }
}
