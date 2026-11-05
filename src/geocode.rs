//! Place lookup through Open-Meteo's geocoding API.
//!
//! The search is the only place where OmaGrow sends a user's typing to the network. It is behind
//! the online switch, needs no key, and returns coordinates, a region and a timezone. Location
//! data comes from GeoNames; weather data from Open-Meteo, both CC BY 4.0.

use std::time::Duration;

use crate::store::Location;

const ENDPOINT: &str = "https://geocoding-api.open-meteo.com/v1/search";
const MAX_RESULTS: usize = 8;

/// One place from the search, already reduced to what the app stores.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Place {
    pub name: String,
    pub region: Option<String>,
    pub country: Option<String>,
    pub timezone: Option<String>,
    pub latitude: f64,
    pub longitude: f64,
}

impl Place {
    /// "Lisbon, Portugal"; the same formatting the stored location uses.
    pub fn label(&self) -> String {
        self.clone()
            .into_location()
            .label()
            .unwrap_or_else(|| self.name.clone())
    }

    pub fn into_location(self) -> Location {
        Location {
            latitude: self.latitude,
            longitude: self.longitude,
            name: Some(self.name),
            region: self.region,
            country: self.country,
            timezone: self.timezone,
        }
    }
}

/// Searches by place name. An empty or one-letter query returns nothing locally.
pub fn search(query: &str) -> Result<Vec<Place>, String> {
    let query = query.trim();
    if query.chars().count() < 2 {
        return Ok(Vec::new());
    }

    let url = format!(
        "{ENDPOINT}?name={}&count={MAX_RESULTS}&language=en&format=json",
        encode(query)
    );
    let body = get(&url)?;
    parse(&body)
}

fn get(url: &str) -> Result<String, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .user_agent(concat!("omagrow/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();

    let mut response = agent.get(url).call().map_err(|error| match error {
        ureq::Error::StatusCode(code) => format!("Open-Meteo answered with HTTP {code}"),
        ureq::Error::Timeout(_) => "Open-Meteo did not answer in time".to_string(),
        other => format!("could not reach Open-Meteo: {other}"),
    })?;

    response
        .body_mut()
        .read_to_string()
        .map_err(|error| format!("could not read the Open-Meteo answer: {error}"))
}

fn parse(body: &str) -> Result<Vec<Place>, String> {
    let value: serde_json::Value = serde_json::from_str(body)
        .map_err(|_| "Open-Meteo returned an unexpected answer".to_string())?;

    let Some(results) = value.get("results").and_then(|results| results.as_array()) else {
        return Ok(Vec::new());
    };

    Ok(results.iter().filter_map(parse_place).collect())
}

fn parse_place(value: &serde_json::Value) -> Option<Place> {
    let name = value.get("name")?.as_str()?.trim();
    if name.is_empty() {
        return None;
    }

    Some(Place {
        name: name.to_string(),
        // admin2 is usually the province or county; admin1 the state or region.
        region: text(value, "admin2").or_else(|| text(value, "admin1")),
        country: text(value, "country"),
        timezone: text(value, "timezone"),
        latitude: value.get("latitude")?.as_f64()?,
        longitude: value.get("longitude")?.as_f64()?,
    })
}

fn text(value: &serde_json::Value, key: &str) -> Option<String> {
    let text = value.get(key)?.as_str()?.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// Percent-encodes a query value; the endpoint accepts `+` for spaces but `%20` is unambiguous.
fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());

    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::{Place, encode, parse, search};

    const ANSWER: &str = r#"{
        "results": [
            {
                "id": 2267057,
                "name": "Lisbon",
                "latitude": 38.7223,
                "longitude": -9.1393,
                "country": "Portugal",
                "admin1": "Lisbon",
                "admin2": "Lisbon District",
                "timezone": "Europe/Lisbon"
            },
            {
                "id": 2735943,
                "name": "Porto",
                "latitude": 41.1496,
                "longitude": -8.611,
                "country": "Portugal",
                "admin1": "Porto",
                "timezone": "Europe/Lisbon"
            }
        ]
    }"#;

    #[test]
    fn reads_the_first_result_with_its_region_and_timezone() {
        let places = parse(ANSWER).unwrap();

        assert_eq!(places.len(), 2);
        assert_eq!(places[0].name, "Lisbon");
        assert_eq!(places[0].region.as_deref(), Some("Lisbon District"));
        assert_eq!(places[0].country.as_deref(), Some("Portugal"));
        assert_eq!(places[0].timezone.as_deref(), Some("Europe/Lisbon"));
        assert_eq!(places[0].latitude, 38.7223);
        assert_eq!(places[0].longitude, -9.1393);
    }

    #[test]
    fn falls_back_to_admin1_without_a_province() {
        let places = parse(ANSWER).unwrap();
        assert_eq!(places[1].region.as_deref(), Some("Porto"));
    }

    #[test]
    fn labels_without_repeating_the_name() {
        let place = Place {
            name: "Berlin".to_string(),
            region: Some("Berlin".to_string()),
            country: Some("Germany".to_string()),
            ..Place::default()
        };

        assert_eq!(place.label(), "Berlin, Germany");
    }

    #[test]
    fn an_empty_answer_has_no_places() {
        assert!(parse("{}").unwrap().is_empty());
        assert!(parse(r#"{"results": []}"#).unwrap().is_empty());
        assert!(parse("not json").is_err());
    }

    #[test]
    fn encodes_spaces_and_accents() {
        assert_eq!(encode("Sao-Paulo"), "Sao-Paulo");
        assert_eq!(encode("Zürich, Schweiz"), "Z%C3%BCrich%2C%20Schweiz");
    }

    #[test]
    fn a_short_query_does_not_hit_the_network() {
        assert!(search("").unwrap().is_empty());
        assert!(search(" G ").unwrap().is_empty());
    }
}
