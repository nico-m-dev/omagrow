//! Seed definitions for the long-tail attribute registry.
//!
//! Definitions are inserted with `INSERT OR IGNORE`, so adding a key here is
//! enough to make it available the next time the app opens.

pub struct AttributeSeed {
    pub key: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub value_type: &'static str,
    pub unit: Option<&'static str>,
    pub enum_values: Option<&'static str>,
}

/// The groups the long tail is shown in. The order here is the order on the profile page.
pub const ATTRIBUTE_CATEGORIES: &[&str] = &[
    "About",
    "Climate",
    "Soil & nutrition",
    "Propagation",
    "Care & training",
    "Harvest & storage",
    "Problems",
    "Garden & use",
    "Other",
];

/// Which group a registered key belongs to. The registry stays the single source of truth; this
/// mapping is applied when the seeds are written, so an existing database picks up a changed
/// category on the next start.
pub fn category_for(key: &str) -> &'static str {
    match key {
        "synonyms"
        | "uses"
        | "allergens"
        | "native_range"
        | "lifespan_years"
        | "mature_height_cm"
        | "mature_spread_cm"
        | "years_to_first_harvest" => "About",

        "heat_tolerance_max_c"
        | "climate_type"
        | "chill_hours"
        | "drought_tolerance"
        | "waterlogging_tolerance"
        | "salt_tolerance"
        | "wind_tolerance"
        | "shade_tolerance"
        | "rainfall_need_mm"
        | "shelter_need"
        | "frost_protection"
        | "winter_protection"
        | "light_spectrum_notes"
        | "artificial_light_suitable"
        | "thermoperiod_notes"
        | "vernalization_temp_max_c"
        | "vernalization_days"
        | "dif_c"
        | "gdd_base_c"
        | "bbch_range"
        | "alternate_bearing" => "Climate",

        "root_depth_cm"
        | "soil_texture"
        | "soil_drainage"
        | "soil_organic_matter_pct"
        | "fertility_need"
        | "root_restriction_tolerance"
        | "substrate_recipe"
        | "mulch_recommended"
        | "mycorrhiza"
        | "rotation_group"
        | "n_fixation" => "Soil & nutrition",

        "propagation_methods"
        | "seed_depth_cm"
        | "germination_light"
        | "stratification_days"
        | "stratification_temp_c"
        | "scarification"
        | "soak_hours"
        | "seed_viability_years"
        | "transplant_age_weeks"
        | "transplant_tolerance"
        | "hardening_off_days"
        | "planting_depth_cm"
        | "spacing_in_row_cm"
        | "spacing_between_rows_cm"
        | "plants_per_m2"
        | "graft_rootstock"
        | "graft_compatibility"
        | "seed_saving_isolation_distance_m" => "Propagation",

        "pruning_wood_age"
        | "pruning_method"
        | "pruning_intensity"
        | "pruning_purpose"
        | "pruning_aftercare"
        | "training_method"
        | "support_type"
        | "pollinator_partner"
        | "flowering_group"
        | "repot_interval_months"
        | "pot_bound_sensitivity" => "Care & training",

        "harvest_parts" | "harvest_indicators" | "harvest_method" | "yield_per_plant"
        | "yield_per_m2" | "storage_temp_c" | "storage_rh_pct" | "shelf_life_days"
        | "processing" | "trichome_harvest" | "flush_days" => "Harvest & storage",

        "pest_susceptibility"
        | "disease_susceptibility"
        | "sensitive_to"
        | "water_quality_sensitivity"
        | "allelopathy" => "Problems",

        "flower_color"
        | "flower_scent"
        | "cut_flower_suitable"
        | "landscape_use"
        | "companions_good"
        | "companions_bad"
        | "mowing_height"
        | "seeding_rate" => "Garden & use",

        _ => "Other",
    }
}

pub const ATTRIBUTE_SEEDS: &[AttributeSeed] = &[
    AttributeSeed {
        key: "synonyms",
        label: "Synonyms",
        description: "Other names, trade names and old names.",
        value_type: "text",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "uses",
        label: "Uses",
        description: "Culinary, medicinal, ornamental, cut flower, dye, fibre, cover crop.",
        value_type: "text",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "allergens",
        label: "Allergens",
        description: "Pollen, sap, spines and other irritants.",
        value_type: "text",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "mature_height_cm",
        label: "Mature height",
        description: "Typical height at maturity.",
        value_type: "number",
        unit: Some("cm"),
        enum_values: None,
    },
    AttributeSeed {
        key: "mature_spread_cm",
        label: "Mature spread",
        description: "Typical spread at maturity.",
        value_type: "number",
        unit: Some("cm"),
        enum_values: None,
    },
    AttributeSeed {
        key: "lifespan_years",
        label: "Lifespan",
        description: "Typical lifespan.",
        value_type: "number",
        unit: Some("years"),
        enum_values: None,
    },
    AttributeSeed {
        key: "years_to_first_harvest",
        label: "Years to first harvest",
        description: "Age at which a perennial first produces a harvest.",
        value_type: "number",
        unit: Some("years"),
        enum_values: None,
    },
    AttributeSeed {
        key: "heat_tolerance_max_c",
        label: "Maximum heat",
        description: "Temperature above which growth or fruit set is damaged.",
        value_type: "number",
        unit: Some("°C"),
        enum_values: None,
    },
    AttributeSeed {
        key: "climate_type",
        label: "Climate type",
        description: "The climate the plant is adapted to.",
        value_type: "enum",
        unit: None,
        enum_values: Some("tropical,subtropical,temperate,mediterranean,arid,continental"),
    },
    AttributeSeed {
        key: "chill_hours",
        label: "Chill hours",
        description: "Hours below the chilling threshold needed to break dormancy.",
        value_type: "number",
        unit: Some("hours"),
        enum_values: None,
    },
    AttributeSeed {
        key: "drought_tolerance",
        label: "Drought tolerance",
        description: "Ability to survive dry periods.",
        value_type: "enum",
        unit: None,
        enum_values: Some("low,medium,high"),
    },
    AttributeSeed {
        key: "waterlogging_tolerance",
        label: "Waterlogging tolerance",
        description: "Ability to survive waterlogged roots.",
        value_type: "enum",
        unit: None,
        enum_values: Some("low,medium,high"),
    },
    AttributeSeed {
        key: "salt_tolerance",
        label: "Salt tolerance",
        description: "Ability to tolerate saline soil or irrigation water.",
        value_type: "enum",
        unit: None,
        enum_values: Some("low,medium,high"),
    },
    AttributeSeed {
        key: "wind_tolerance",
        label: "Wind tolerance",
        description: "Ability to tolerate exposed, windy sites.",
        value_type: "enum",
        unit: None,
        enum_values: Some("low,medium,high"),
    },
    AttributeSeed {
        key: "shade_tolerance",
        label: "Shade tolerance",
        description: "Ability to grow in less than full sun.",
        value_type: "enum",
        unit: None,
        enum_values: Some("low,medium,high"),
    },
    AttributeSeed {
        key: "native_range",
        label: "Native range",
        description: "Geographic origin of the species.",
        value_type: "text",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "rainfall_need_mm",
        label: "Rainfall need",
        description: "Approximate annual or seasonal water need.",
        value_type: "number",
        unit: Some("mm"),
        enum_values: None,
    },
    AttributeSeed {
        key: "shelter_need",
        label: "Shelter need",
        description: "Windbreak, shade cloth, frost cover and similar protection.",
        value_type: "text",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "root_depth_cm",
        label: "Root depth",
        description: "Typical effective root depth.",
        value_type: "number",
        unit: Some("cm"),
        enum_values: None,
    },
    AttributeSeed {
        key: "soil_texture",
        label: "Soil texture",
        description: "Preferred soil texture: sand, loam, silt, clay or a mixture.",
        value_type: "text",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "soil_drainage",
        label: "Soil drainage",
        description: "Preferred drainage.",
        value_type: "enum",
        unit: None,
        enum_values: Some("poor,moderate,free"),
    },
    AttributeSeed {
        key: "soil_organic_matter_pct",
        label: "Soil organic matter",
        description: "Preferred organic matter content.",
        value_type: "number",
        unit: Some("%"),
        enum_values: None,
    },
    AttributeSeed {
        key: "fertility_need",
        label: "Fertility need",
        description: "Overall nutrient demand.",
        value_type: "enum",
        unit: None,
        enum_values: Some("low,medium,high"),
    },
    AttributeSeed {
        key: "root_restriction_tolerance",
        label: "Root restriction tolerance",
        description: "Ability to grow in a container.",
        value_type: "enum",
        unit: None,
        enum_values: Some("low,medium,high"),
    },
    AttributeSeed {
        key: "substrate_recipe",
        label: "Substrate recipe",
        description: "Container mix or hydroponic substrate, for example peat/coco/perlite.",
        value_type: "text",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "mulch_recommended",
        label: "Mulch recommended",
        description: "Whether mulching is useful.",
        value_type: "boolean",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "mycorrhiza",
        label: "Mycorrhiza",
        description: "Whether mycorrhizal inoculation is useful.",
        value_type: "boolean",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "propagation_methods",
        label: "Propagation methods",
        description: "Seed, cutting, division, layering, tuber, bulb or grafting.",
        value_type: "text",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "seed_depth_cm",
        label: "Seed depth",
        description: "Sowing depth.",
        value_type: "number",
        unit: Some("cm"),
        enum_values: None,
    },
    AttributeSeed {
        key: "germination_light",
        label: "Germination light",
        description: "Light needed for germination.",
        value_type: "enum",
        unit: None,
        enum_values: Some("light,dark,either"),
    },
    AttributeSeed {
        key: "stratification_days",
        label: "Stratification",
        description: "Cold treatment before sowing.",
        value_type: "number",
        unit: Some("days"),
        enum_values: None,
    },
    AttributeSeed {
        key: "stratification_temp_c",
        label: "Stratification temperature",
        description: "Temperature for cold treatment.",
        value_type: "number",
        unit: Some("°C"),
        enum_values: None,
    },
    AttributeSeed {
        key: "scarification",
        label: "Scarification",
        description: "Mechanical or thermal seed treatment.",
        value_type: "text",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "soak_hours",
        label: "Soak time",
        description: "Pre-sowing soak.",
        value_type: "number",
        unit: Some("hours"),
        enum_values: None,
    },
    AttributeSeed {
        key: "seed_viability_years",
        label: "Seed viability",
        description: "Storage life of the seed.",
        value_type: "number",
        unit: Some("years"),
        enum_values: None,
    },
    AttributeSeed {
        key: "transplant_age_weeks",
        label: "Transplant age",
        description: "Age at which seedlings are transplanted.",
        value_type: "number",
        unit: Some("weeks"),
        enum_values: None,
    },
    AttributeSeed {
        key: "transplant_tolerance",
        label: "Transplant tolerance",
        description: "Tolerance of root disturbance.",
        value_type: "enum",
        unit: None,
        enum_values: Some("low,medium,high"),
    },
    AttributeSeed {
        key: "hardening_off_days",
        label: "Hardening off",
        description: "Days needed to acclimatise before planting out.",
        value_type: "number",
        unit: Some("days"),
        enum_values: None,
    },
    AttributeSeed {
        key: "planting_depth_cm",
        label: "Planting depth",
        description: "Depth for transplants, tubers or bulbs.",
        value_type: "number",
        unit: Some("cm"),
        enum_values: None,
    },
    AttributeSeed {
        key: "spacing_in_row_cm",
        label: "Spacing in row",
        description: "Distance between plants in a row.",
        value_type: "number",
        unit: Some("cm"),
        enum_values: None,
    },
    AttributeSeed {
        key: "spacing_between_rows_cm",
        label: "Spacing between rows",
        description: "Distance between rows.",
        value_type: "number",
        unit: Some("cm"),
        enum_values: None,
    },
    AttributeSeed {
        key: "plants_per_m2",
        label: "Plants per m²",
        description: "Planting density.",
        value_type: "number",
        unit: Some("plants/m²"),
        enum_values: None,
    },
    AttributeSeed {
        key: "graft_rootstock",
        label: "Graft rootstock",
        description: "Rootstock used for grafting.",
        value_type: "text",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "graft_compatibility",
        label: "Graft compatibility",
        description: "Notes on compatible rootstocks or scions.",
        value_type: "text",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "seed_saving_isolation_distance_m",
        label: "Seed saving isolation",
        description: "Isolation distance for seed saving.",
        value_type: "number",
        unit: Some("m"),
        enum_values: None,
    },
    AttributeSeed {
        key: "pruning_wood_age",
        label: "Pruning wood age",
        description: "Whether the plant flowers on old or new wood.",
        value_type: "enum",
        unit: None,
        enum_values: Some("old,new,both"),
    },
    AttributeSeed {
        key: "pruning_method",
        label: "Pruning method",
        description: "Heading, thinning, topping, renewal or deadheading.",
        value_type: "text",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "pruning_intensity",
        label: "Pruning intensity",
        description: "How hard the plant can be pruned.",
        value_type: "enum",
        unit: None,
        enum_values: Some("light,moderate,hard"),
    },
    AttributeSeed {
        key: "pruning_purpose",
        label: "Pruning purpose",
        description: "Shape, airflow, yield or disease removal.",
        value_type: "text",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "pruning_aftercare",
        label: "Pruning aftercare",
        description: "Aftercare after pruning.",
        value_type: "text",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "training_method",
        label: "Training method",
        description: "Stake, trellis, LST, SCROG, espalier and similar.",
        value_type: "text",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "support_type",
        label: "Support type",
        description: "None, stake, cage or trellis.",
        value_type: "text",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "pollinator_partner",
        label: "Pollinator partner",
        description: "Required cross-pollinator.",
        value_type: "text",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "flowering_group",
        label: "Flowering group",
        description: "Flowering group for fruit trees.",
        value_type: "text",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "winter_protection",
        label: "Winter protection",
        description: "Mulch, fleece, move indoors and similar.",
        value_type: "text",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "repot_interval_months",
        label: "Repot interval",
        description: "Months between repotting.",
        value_type: "number",
        unit: Some("months"),
        enum_values: None,
    },
    AttributeSeed {
        key: "pot_bound_sensitivity",
        label: "Pot-bound sensitivity",
        description: "How quickly the plant suffers from being pot-bound.",
        value_type: "enum",
        unit: None,
        enum_values: Some("low,medium,high"),
    },
    AttributeSeed {
        key: "rotation_group",
        label: "Rotation group",
        description: "Crop rotation family or group.",
        value_type: "text",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "harvest_parts",
        label: "Harvest parts",
        description: "Leaf, fruit, root, seed, flower, stem, tuber or bulb.",
        value_type: "text",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "harvest_indicators",
        label: "Harvest indicators",
        description: "Colour, firmness, size, days, trichomes or scent.",
        value_type: "text",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "harvest_method",
        label: "Harvest method",
        description: "Cut, pick, dig or pull.",
        value_type: "text",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "yield_per_plant",
        label: "Yield per plant",
        description: "Typical harvest per plant.",
        value_type: "number",
        unit: Some("kg/plant"),
        enum_values: None,
    },
    AttributeSeed {
        key: "yield_per_m2",
        label: "Yield per m²",
        description: "Typical harvest per square metre.",
        value_type: "number",
        unit: Some("kg/m²"),
        enum_values: None,
    },
    AttributeSeed {
        key: "storage_temp_c",
        label: "Storage temperature",
        description: "Recommended storage temperature.",
        value_type: "number",
        unit: Some("°C"),
        enum_values: None,
    },
    AttributeSeed {
        key: "storage_rh_pct",
        label: "Storage humidity",
        description: "Recommended storage relative humidity.",
        value_type: "number",
        unit: Some("%"),
        enum_values: None,
    },
    AttributeSeed {
        key: "shelf_life_days",
        label: "Shelf life",
        description: "Typical storage life.",
        value_type: "number",
        unit: Some("days"),
        enum_values: None,
    },
    AttributeSeed {
        key: "processing",
        label: "Processing",
        description: "Drying, curing, freezing or canning.",
        value_type: "text",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "flower_color",
        label: "Flower colour",
        description: "Ornamental flower colour.",
        value_type: "text",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "flower_scent",
        label: "Flower scent",
        description: "Ornamental scent.",
        value_type: "text",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "cut_flower_suitable",
        label: "Cut flower suitable",
        description: "Whether the plant works as a cut flower.",
        value_type: "boolean",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "landscape_use",
        label: "Landscape use",
        description: "Border, ground cover, hedge or specimen.",
        value_type: "text",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "companions_good",
        label: "Good companions",
        description: "Species that grow well nearby and why.",
        value_type: "text",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "companions_bad",
        label: "Bad companions",
        description: "Species that should not grow nearby and why.",
        value_type: "text",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "allelopathy",
        label: "Allelopathy",
        description: "Whether the plant suppresses neighbouring plants.",
        value_type: "boolean",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "pest_susceptibility",
        label: "Pest susceptibility",
        description: "Common pests and conditions that favour them.",
        value_type: "text",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "disease_susceptibility",
        label: "Disease susceptibility",
        description: "Common diseases and conditions that favour them.",
        value_type: "text",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "sensitive_to",
        label: "Sensitive to",
        description: "Foot traffic, pollution, salt, wind and similar.",
        value_type: "text",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "mowing_height",
        label: "Mowing height",
        description: "Recommended mowing height.",
        value_type: "number",
        unit: Some("cm"),
        enum_values: None,
    },
    AttributeSeed {
        key: "seeding_rate",
        label: "Seeding rate",
        description: "Recommended seeding rate.",
        value_type: "number",
        unit: Some("g/m²"),
        enum_values: None,
    },
    AttributeSeed {
        key: "n_fixation",
        label: "Nitrogen fixation",
        description: "Whether the plant fixes atmospheric nitrogen.",
        value_type: "boolean",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "trichome_harvest",
        label: "Trichome harvest",
        description: "Harvest indicator for resinous flowers.",
        value_type: "text",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "flush_days",
        label: "Flush",
        description: "Days of flushing before harvest, where applicable.",
        value_type: "number",
        unit: Some("days"),
        enum_values: None,
    },
    AttributeSeed {
        key: "bbch_range",
        label: "BBCH range",
        description: "Optional BBCH stage range for agents.",
        value_type: "text",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "gdd_base_c",
        label: "GDD base temperature",
        description: "Base temperature for growing degree days.",
        value_type: "number",
        unit: Some("°C"),
        enum_values: None,
    },
    AttributeSeed {
        key: "vernalization_temp_max_c",
        label: "Vernalisation temperature",
        description: "Maximum temperature for vernalisation.",
        value_type: "number",
        unit: Some("°C"),
        enum_values: None,
    },
    AttributeSeed {
        key: "vernalization_days",
        label: "Vernalisation days",
        description: "Duration at vernalising temperature.",
        value_type: "number",
        unit: Some("days"),
        enum_values: None,
    },
    AttributeSeed {
        key: "alternate_bearing",
        label: "Alternate bearing",
        description: "Whether the plant crops heavily one year and lightly the next.",
        value_type: "boolean",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "dif_c",
        label: "Day–night difference",
        description: "Preferred difference between day and night temperature.",
        value_type: "number",
        unit: Some("°C"),
        enum_values: None,
    },
    AttributeSeed {
        key: "thermoperiod_notes",
        label: "Thermoperiod notes",
        description: "Qualitative notes about day and night temperature.",
        value_type: "text",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "frost_protection",
        label: "Frost protection",
        description: "Methods and thresholds for frost protection.",
        value_type: "text",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "water_quality_sensitivity",
        label: "Water quality sensitivity",
        description: "Chlorine, hardness, salinity and similar.",
        value_type: "text",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "light_spectrum_notes",
        label: "Light spectrum notes",
        description: "Blue/red ratio, UV and similar.",
        value_type: "text",
        unit: None,
        enum_values: None,
    },
    AttributeSeed {
        key: "artificial_light_suitable",
        label: "Artificial light suitable",
        description: "Whether the plant performs indoors under lamps.",
        value_type: "boolean",
        unit: None,
        enum_values: None,
    },
];

#[cfg(test)]
mod tests {
    use super::{ATTRIBUTE_CATEGORIES, ATTRIBUTE_SEEDS, category_for};

    #[test]
    fn every_seed_has_a_known_category() {
        for seed in ATTRIBUTE_SEEDS {
            let category = category_for(seed.key);
            assert!(
                ATTRIBUTE_CATEGORIES.contains(&category),
                "{} has unknown category {category}",
                seed.key
            );
        }
    }
}
