//! The space illustrations, grouped by the environment they belong to.
//!
//! The order is the order the picker shows: outdoor places first, then greenhouse structures,
//! then indoor growing. Slugs name a picture, not the space's `kind`; the kind only decides the
//! fallback when no illustration is chosen.

pub(in crate::ui) const SPACES: &[&str] = &[
    "garden-bed",
    "raised-bed",
    "balcony",
    "patio",
    "orchard",
    "greenhouse",
    "polytunnel",
    "cold-frame",
    "windowsill",
    "indoor-shelf",
    "grow-tent",
    "hydroponics",
];

/// The illustration a space gets before the user picks one. `kind` is the space's physical kind:
/// `outdoor`, `greenhouse` or `indoor`.
pub(in crate::ui) fn default_for_kind(kind: Option<&str>) -> &'static str {
    match kind {
        Some("greenhouse") => "greenhouse",
        Some("indoor") => "indoor-shelf",
        _ => "garden-bed",
    }
}

/// A readable label from the slug: "Cold frame", "Grow tent".
pub(in crate::ui) fn label(slug: &str) -> String {
    let text = slug.replace('-', " ");
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{SPACES, default_for_kind, label};

    #[test]
    fn labels_read_as_places() {
        assert_eq!(label("garden-bed"), "Garden bed");
        assert_eq!(label("cold-frame"), "Cold frame");
        assert_eq!(label("polytunnel"), "Polytunnel");
    }

    #[test]
    fn every_kind_falls_back_to_a_bundled_picture() {
        for kind in crate::planting::SPACE_KINDS
            .iter()
            .copied()
            .map(Some)
            .chain([None])
        {
            assert!(
                SPACES.contains(&default_for_kind(kind)),
                "no bundled fallback for {kind:?}"
            );
        }
    }
}
