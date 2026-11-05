//! The user's switch for Open-Meteo.
//!
//! OmaGrow uses the network for a place search, a frost estimate and the background weather sync.
//! This switch gates all three, so turning it off really means "make no requests"; the coordinates
//! and frost dates stay editable by hand and stored weather stays visible. The setting lives in the
//! settings table; a missing value means on, so a fresh install can search a city without hunting
//! for a toggle first.

use crate::store::{self, Store};

const KEY: &str = "online_data";

pub fn enabled(store: &Store) -> bool {
    !matches!(store.setting(KEY).ok().flatten().as_deref(), Some("off"))
}

pub fn set_enabled(store: &Store, enabled: bool) -> store::Result<()> {
    store.set_setting(KEY, if enabled { "on" } else { "off" })
}

#[cfg(test)]
mod tests {
    use super::{enabled, set_enabled};
    use crate::store::Store;

    #[test]
    fn is_on_until_the_user_turns_it_off() {
        let store = Store::open_in_memory().unwrap();
        assert!(enabled(&store));

        set_enabled(&store, false).unwrap();
        assert!(!enabled(&store));

        set_enabled(&store, true).unwrap();
        assert!(enabled(&store));
    }
}
