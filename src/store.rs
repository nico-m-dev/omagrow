//! Local storage for plant profiles.
//!
//! The database lives in the XDG data directory (`~/.local/share/omagrow/omagrow.db`).
//! Schema changes go through `PRAGMA user_version`; add a new step to `migrate` instead of
//! editing an existing one.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use crate::attribute_seeds::{ATTRIBUTE_CATEGORIES, ATTRIBUTE_SEEDS, category_for};
use crate::changes::Change;
use crate::climate::{ClimateInput, ClimateReading};
use crate::media::Media;
use crate::observation::{Observation, ObservationInput, parse_time};
use crate::plant_data::{
    APPLIES_TO, Attribute, AttributeDefinition, CONFIDENCE_LEVELS, DAY_PARTS, DETERMINACY, FOLIAGE,
    GROWTH_HABITS, LIFE_FORMS, LIFECYCLES, METRICS, PHOTOPERIOD_TYPES, PROVENANCE_STATUSES,
    ProfileDetails, Provenance, Requirement, RequirementResolution, SOURCE_TYPES, STAGE_KINDS,
    SUN_EXPOSURES, Stage, TIMING_ANCHORS, TIMING_KINDS, TimingRule, VALUE_TYPES, ValidationError,
    WATER_NEEDS, validate_choice, validate_non_negative, validate_optional_choice,
    validate_optional_range, validate_requirement_range,
};
use crate::planting::{
    Planting, PlantingInput, PlantingPlacement, PlantingSummary, Space, SpaceInput,
    validate_outcome,
};
use crate::weather::{WeatherCurrent, WeatherDay, WeatherHour};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profile {
    pub id: i64,
    pub name: String,
    pub species: Option<String>,
    pub cultivar: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Location {
    pub latitude: f64,
    pub longitude: f64,
    /// Keep the place name optional: a coordinate set by hand has none.
    pub name: Option<String>,
    pub region: Option<String>,
    pub country: Option<String>,
    pub timezone: Option<String>,
}

impl Location {
    /// "Lisbon, Portugal" for a searched place, `None` for hand-entered coordinates.
    pub fn label(&self) -> Option<String> {
        let name = self.name.as_deref()?.trim();
        if name.is_empty() {
            return None;
        }

        let mut parts = vec![name.to_string()];

        for part in [self.region.as_deref(), self.country.as_deref()] {
            if let Some(part) = part.map(str::trim)
                && !part.is_empty()
                && !parts.iter().any(|known| known == part)
            {
                parts.push(part.to_string());
            }
        }

        Some(parts.join(", "))
    }
}

pub struct Store {
    conn: Connection,
}

impl Store {
    /// The schema this build writes. A read-only client compares it to warn about an old file.
    pub const SCHEMA_VERSION: i64 = 17;

    pub fn open_default() -> Result<Self> {
        let dir = match data_dir() {
            Some(dir) => dir,
            None => return Err(no_data_dir()),
        };
        fs::create_dir_all(&dir)?;
        Self::open(dir.join("omagrow.db"))
    }

    /// The database the app uses, or `None` when neither `XDG_DATA_HOME` nor `HOME` is set.
    pub fn default_path() -> Option<PathBuf> {
        data_dir().map(|dir| dir.join("omagrow.db"))
    }

    /// Opens the database without migrating, seeding or writing anything.
    ///
    /// For readers such as the agent CLI: a missing file is an error instead of a new database,
    /// and a read-only file or directory still works.
    pub fn open_read_only() -> Result<Self> {
        let path = match Self::default_path() {
            Some(path) => path,
            None => return Err(no_data_dir()),
        };

        let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        conn.busy_timeout(Duration::from_secs(5))?;

        Ok(Self { conn })
    }

    /// Opens the database for writing without migrating or seeding. For a command that writes,
    /// such as a proposal import; a schema of another version is refused by the caller.
    pub fn open_read_write() -> Result<Self> {
        let path = match Self::default_path() {
            Some(path) => path,
            None => return Err(no_data_dir()),
        };

        let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        // The app runs with foreign keys on; the importer has to behave the same way.
        conn.pragma_update(None, "foreign_keys", "ON")?;

        Ok(Self { conn })
    }

    pub fn schema_version(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))?)
    }

    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value)
             VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET
                 value = excluded.value,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')",
            params![key, value],
        )?;

        Ok(())
    }

    /// Runs `run` in one transaction: a failure rolls everything back.
    pub fn transaction<T>(&self, run: impl FnOnce(&Store) -> Result<T>) -> Result<T> {
        let transaction = self.conn.unchecked_transaction()?;
        let value = run(self)?;
        transaction.commit()?;
        Ok(value)
    }

    /// Runs `run` in a transaction and throws the result away, so validation can happen without
    /// writing anything.
    pub fn dry_run<T>(&self, run: impl FnOnce(&Store) -> Result<T>) -> Result<T> {
        let transaction = self.conn.unchecked_transaction()?;
        let value = run(self)?;
        transaction.rollback()?;
        Ok(value)
    }

    /// Runs statements atomically, or inside the caller's transaction when there already is one.
    /// Nested `BEGIN` is an error in SQLite, so a proposal's transaction needs this.
    fn atomically(&self, run: impl FnOnce(&Connection) -> Result<()>) -> Result<()> {
        if self.conn.is_autocommit() {
            let transaction = self.conn.unchecked_transaction()?;
            run(&transaction)?;
            transaction.commit()?;
        } else {
            run(&self.conn)?;
        }

        Ok(())
    }

    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self> {
        Self::prepare(Connection::open(path)?)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self> {
        Self::prepare(Connection::open_in_memory()?)
    }

    fn prepare(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(Duration::from_secs(5))?;

        let store = Self { conn };
        store.migrate()?;
        store.seed_attribute_definitions()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<()> {
        let version: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;

        if version < 1 {
            self.conn.execute_batch(
                "BEGIN;
                 CREATE TABLE profiles (
                     id         INTEGER PRIMARY KEY AUTOINCREMENT,
                     name       TEXT NOT NULL,
                     latin_name TEXT,
                     cultivar   TEXT,
                     created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                     updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
                 );
                 PRAGMA user_version = 1;
                 COMMIT;",
            )?;
        }

        if version < 2 {
            self.conn.execute_batch(
                "BEGIN;
                 CREATE TABLE locations (
                     id         INTEGER PRIMARY KEY CHECK (id = 1),
                     latitude   REAL NOT NULL,
                     longitude  REAL NOT NULL,
                     updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
                 );
                 PRAGMA user_version = 2;
                 COMMIT;",
            )?;
        }

        if version < 3 {
            self.conn.execute_batch(
                "BEGIN;
                 ALTER TABLE profiles RENAME COLUMN latin_name TO species;
                 PRAGMA user_version = 3;
                 COMMIT;",
            )?;
        }

        if version < 4 {
            self.conn.execute_batch(
                "BEGIN;
                 ALTER TABLE profiles ADD COLUMN family TEXT;
                 ALTER TABLE profiles ADD COLUMN genus TEXT;
                 ALTER TABLE profiles ADD COLUMN lifecycle TEXT;
                 ALTER TABLE profiles ADD COLUMN life_form TEXT;
                 ALTER TABLE profiles ADD COLUMN foliage TEXT;
                 ALTER TABLE profiles ADD COLUMN determinacy TEXT;
                 ALTER TABLE profiles ADD COLUMN growth_habit TEXT;
                 ALTER TABLE profiles ADD COLUMN sun_exposure TEXT;
                 ALTER TABLE profiles ADD COLUMN photoperiod_type TEXT;
                 ALTER TABLE profiles ADD COLUMN photoperiod_obligate INTEGER;
                 ALTER TABLE profiles ADD COLUMN critical_day_length_hours REAL;
                 ALTER TABLE profiles ADD COLUMN water_need TEXT;
                 ALTER TABLE profiles ADD COLUMN container_suitable INTEGER;
                 ALTER TABLE profiles ADD COLUMN min_container_volume_l_min REAL;
                 ALTER TABLE profiles ADD COLUMN min_container_volume_l_max REAL;
                 ALTER TABLE profiles ADD COLUMN days_to_maturity_min REAL;
                 ALTER TABLE profiles ADD COLUMN days_to_maturity_max REAL;
                 ALTER TABLE profiles ADD COLUMN notes TEXT;

                 CREATE TABLE profile_stages (
                     id                INTEGER PRIMARY KEY AUTOINCREMENT,
                     profile_id        INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
                     position          INTEGER NOT NULL,
                     kind              TEXT NOT NULL,
                     label             TEXT NOT NULL,
                     duration_days_min REAL,
                     duration_days_max REAL,
                     notes             TEXT,
                     created_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                     updated_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                     UNIQUE(profile_id, position),
                     UNIQUE(profile_id, id)
                 );

                 CREATE TABLE profile_requirements (
                     id            INTEGER PRIMARY KEY AUTOINCREMENT,
                     profile_id    INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
                     stage_id      INTEGER,
                     metric        TEXT NOT NULL,
                     day_part      TEXT NOT NULL,
                     min_value     REAL,
                     max_value     REAL,
                     opt_min       REAL,
                     opt_max       REAL,
                     survival_min  REAL,
                     survival_max  REAL,
                     created_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                     updated_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                     FOREIGN KEY (profile_id, stage_id)
                         REFERENCES profile_stages(profile_id, id) ON DELETE CASCADE
                 );

                 CREATE TABLE profile_timing_rules (
                     id               INTEGER PRIMARY KEY AUTOINCREMENT,
                     profile_id       INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
                     stage_id         INTEGER,
                     kind             TEXT NOT NULL,
                     anchor           TEXT NOT NULL,
                     offset_min_days  INTEGER,
                     offset_max_days  INTEGER,
                     month_start      INTEGER,
                     month_end        INTEGER,
                     day_length_hours REAL,
                     gdd_base_c       REAL,
                     gdd_target       REAL,
                     repeat_days      INTEGER,
                     notes            TEXT,
                     created_at       TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                     updated_at       TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                     FOREIGN KEY (profile_id, stage_id)
                         REFERENCES profile_stages(profile_id, id) ON DELETE CASCADE
                 );

                 CREATE TABLE attribute_definitions (
                     key                TEXT PRIMARY KEY,
                     label              TEXT NOT NULL,
                     description        TEXT,
                     value_type         TEXT NOT NULL,
                     unit               TEXT,
                     applies_to         TEXT NOT NULL DEFAULT 'profile',
                     allowed_stages     TEXT,
                     allowed_lifecycles TEXT,
                     enum_values        TEXT,
                     is_custom          INTEGER NOT NULL DEFAULT 0,
                     created_at         TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
                 );

                 CREATE TABLE profile_attributes (
                     id         INTEGER PRIMARY KEY AUTOINCREMENT,
                     profile_id INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
                     stage_id   INTEGER,
                     key        TEXT NOT NULL REFERENCES attribute_definitions(key),
                     value_num  REAL,
                     value_text TEXT,
                     value_bool INTEGER,
                     created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                     updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                     FOREIGN KEY (profile_id, stage_id)
                         REFERENCES profile_stages(profile_id, id) ON DELETE CASCADE,
                     CHECK ((value_num IS NOT NULL) + (value_text IS NOT NULL) + (value_bool IS NOT NULL) = 1)
                 );

                 -- No foreign key: provenance is polymorphic. delete_* methods clean it up.
                 CREATE TABLE field_provenance (
                     entity_type  TEXT NOT NULL,
                     entity_id    INTEGER NOT NULL,
                     field        TEXT NOT NULL DEFAULT '*',
                     status       TEXT NOT NULL,
                     source_type  TEXT,
                     source_ref   TEXT,
                     retrieved_at TEXT,
                     confidence   TEXT,
                     notes        TEXT,
                     updated_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                     PRIMARY KEY (entity_type, entity_id, field)
                 );

                 CREATE UNIQUE INDEX profile_requirements_unique
                     ON profile_requirements(profile_id, COALESCE(stage_id, 0), metric, day_part);
                 CREATE INDEX profile_requirements_profile
                     ON profile_requirements(profile_id);

                 CREATE UNIQUE INDEX profile_attributes_unique
                     ON profile_attributes(profile_id, COALESCE(stage_id, 0), key);
                 CREATE INDEX profile_attributes_profile
                     ON profile_attributes(profile_id);

                 CREATE INDEX profile_timing_rules_profile
                     ON profile_timing_rules(profile_id);

                 CREATE INDEX field_provenance_entity
                     ON field_provenance(entity_type, entity_id);

                 PRAGMA user_version = 4;
                 COMMIT;",
            )?;
        }

        if version < 5 {
            self.conn.execute_batch(
                "BEGIN;
                 CREATE TABLE media (
                     id         INTEGER PRIMARY KEY AUTOINCREMENT,
                     owner_type TEXT NOT NULL,
                     owner_id   INTEGER NOT NULL,
                     role       TEXT NOT NULL DEFAULT 'cover',
                     file_stem  TEXT NOT NULL,
                     width      INTEGER NOT NULL,
                     height     INTEGER NOT NULL,
                     bytes      INTEGER NOT NULL,
                     created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                     updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
                 );
                 CREATE INDEX media_owner ON media(owner_type, owner_id);

                 PRAGMA user_version = 5;
                 COMMIT;",
            )?;
        }

        if version < 6 {
            self.conn.execute_batch(
                "BEGIN;
                 CREATE TABLE settings (
                     key        TEXT PRIMARY KEY,
                     value      TEXT NOT NULL,
                     updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
                 );

                 PRAGMA user_version = 6;
                 COMMIT;",
            )?;
        }

        if version < 7 {
            self.conn.execute_batch(
                "BEGIN;
                 CREATE TABLE changes (
                     id          INTEGER PRIMARY KEY AUTOINCREMENT,
                     profile_id  INTEGER NOT NULL,
                     entity_type TEXT NOT NULL,
                     entity_id   INTEGER NOT NULL,
                     field       TEXT NOT NULL,
                     old_value   TEXT,
                     new_value   TEXT,
                     source_type TEXT,
                     source_ref  TEXT,
                     actor       TEXT NOT NULL,
                     created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
                 );
                 CREATE INDEX changes_profile ON changes(profile_id, id);

                 PRAGMA user_version = 7;
                 COMMIT;",
            )?;
        }

        if version < 8 {
            self.conn.execute_batch(
                "BEGIN;
                 ALTER TABLE attribute_definitions ADD COLUMN category TEXT;
                 PRAGMA user_version = 8;
                 COMMIT;",
            )?;
        }

        if version < 9 {
            self.conn.execute_batch(
                "BEGIN;
                 ALTER TABLE locations ADD COLUMN name TEXT;
                 ALTER TABLE locations ADD COLUMN region TEXT;
                 ALTER TABLE locations ADD COLUMN country TEXT;
                 ALTER TABLE locations ADD COLUMN timezone TEXT;
                 PRAGMA user_version = 9;
                 COMMIT;",
            )?;
        }

        if version < 10 {
            self.conn.execute_batch(
                "BEGIN;
                 CREATE TABLE spaces (
                     id          INTEGER PRIMARY KEY AUTOINCREMENT,
                     name        TEXT,
                     kind        TEXT NOT NULL DEFAULT 'outdoor',
                     light_type  TEXT,
                     light_hours REAL,
                     notes       TEXT,
                     created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                     updated_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
                 );
                 CREATE UNIQUE INDEX spaces_name
                     ON spaces(name COLLATE NOCASE) WHERE name IS NOT NULL;

                 CREATE TABLE plantings (
                     id                 INTEGER PRIMARY KEY AUTOINCREMENT,
                     profile_id         INTEGER NOT NULL REFERENCES profiles(id) ON DELETE RESTRICT,
                     space_id           INTEGER NOT NULL REFERENCES spaces(id) ON DELETE RESTRICT,
                     name               TEXT,
                     quantity           INTEGER NOT NULL DEFAULT 1,
                     container          TEXT NOT NULL DEFAULT 'ground',
                     container_volume_l REAL,
                     substrate          TEXT,
                     started_on         TEXT NOT NULL,
                     start_kind         TEXT NOT NULL DEFAULT 'planted',
                     ended_on           TEXT,
                     outcome            TEXT,
                     notes              TEXT,
                     created_at         TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                     updated_at         TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
                 );
                 CREATE INDEX plantings_profile ON plantings(profile_id);
                 CREATE INDEX plantings_space ON plantings(space_id);

                 PRAGMA user_version = 10;
                 COMMIT;",
            )?;
        }

        if version < 11 {
            self.conn.execute_batch(
                "BEGIN;
                 CREATE TABLE observations (
                     id              INTEGER PRIMARY KEY AUTOINCREMENT,
                     planting_id     INTEGER NOT NULL REFERENCES plantings(id) ON DELETE CASCADE,
                     kind            TEXT NOT NULL,
                     occurred_on     TEXT NOT NULL,
                     occurred_time   TEXT,
                     note            TEXT,
                     amount          REAL,
                     unit            TEXT,
                     stage_id        INTEGER REFERENCES profile_stages(id) ON DELETE SET NULL,
                     previous_amount REAL,
                     previous_unit   TEXT,
                     created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                     updated_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
                 );
                 CREATE INDEX observations_planting
                     ON observations(planting_id, occurred_on, id);

                 PRAGMA user_version = 11;
                 COMMIT;",
            )?;
        }

        if version < 12 {
            self.conn.execute_batch(
                "BEGIN;
                 CREATE TABLE climate_readings (
                     id              INTEGER PRIMARY KEY AUTOINCREMENT,
                     space_id        INTEGER NOT NULL REFERENCES spaces(id) ON DELETE CASCADE,
                     measured_on     TEXT NOT NULL,
                     measured_time   TEXT,
                     temperature_c   REAL,
                     humidity_pct    REAL,
                     source          TEXT NOT NULL,
                     source_ref      TEXT,
                     created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                     CHECK (temperature_c IS NOT NULL OR humidity_pct IS NOT NULL)
                 );
                 CREATE INDEX climate_readings_space
                     ON climate_readings(space_id, measured_on, measured_time, id);

                 PRAGMA user_version = 12;
                 COMMIT;",
            )?;
        }

        if version < 13 {
            self.conn.execute_batch(
                "BEGIN;
                 CREATE TABLE weather_current (
                     id                     INTEGER PRIMARY KEY AUTOINCREMENT,
                     latitude               REAL NOT NULL,
                     longitude              REAL NOT NULL,
                     observed_on            TEXT NOT NULL,
                     observed_time          TEXT,
                     temperature_c          REAL,
                     humidity_pct           REAL,
                     apparent_temperature_c REAL,
                     weather_code           INTEGER,
                     precipitation_mm       REAL,
                     cloud_cover_pct        REAL,
                     wind_speed_kmh         REAL,
                     wind_gust_kmh          REAL,
                     is_day                 INTEGER,
                     source                 TEXT NOT NULL DEFAULT 'weather',
                     source_ref             TEXT,
                     fetched_at             TEXT NOT NULL
                         DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                     CHECK (temperature_c IS NOT NULL OR humidity_pct IS NOT NULL),
                     CHECK (is_day IS NULL OR is_day IN (0, 1))
                 );
                 CREATE UNIQUE INDEX weather_current_moment
                     ON weather_current(latitude, longitude, observed_on, observed_time);
                 CREATE INDEX weather_current_latest
                     ON weather_current(latitude, longitude, observed_on DESC, observed_time DESC);

                 CREATE TABLE weather_daily (
                     id                   INTEGER PRIMARY KEY AUTOINCREMENT,
                     latitude             REAL NOT NULL,
                     longitude            REAL NOT NULL,
                     date                 TEXT NOT NULL,
                     temp_min_c           REAL,
                     temp_max_c           REAL,
                     temp_mean_c          REAL,
                     humidity_mean_pct    REAL,
                     humidity_min_pct     REAL,
                     humidity_max_pct     REAL,
                     precipitation_mm     REAL,
                     precipitation_hours  REAL,
                     weather_code         INTEGER,
                     sunshine_hours       REAL,
                     daylight_hours       REAL,
                     radiation_sum_mj_m2  REAL,
                     wind_max_kmh         REAL,
                     wind_gust_max_kmh    REAL,
                     et0_mm               REAL,
                     night_start          TEXT,
                     night_end            TEXT,
                     night_temp_avg_c     REAL,
                     night_temp_min_c     REAL,
                     night_humidity_avg_pct REAL,
                     night_hours          REAL,
                     source               TEXT NOT NULL DEFAULT 'weather',
                     source_ref           TEXT,
                     fetched_at           TEXT NOT NULL
                         DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                     UNIQUE(latitude, longitude, date)
                 );
                 CREATE INDEX weather_daily_date
                     ON weather_daily(latitude, longitude, date DESC);

                 -- Weather is model data, not a local reading. Move the rows written by the
                 -- first weather source out of climate_readings, so manual and sensor readings
                 -- stay a separate series.
                 INSERT OR IGNORE INTO weather_current
                     (latitude, longitude, observed_on, observed_time, temperature_c,
                      humidity_pct, source, source_ref, fetched_at)
                 SELECT COALESCE(l.latitude, 0.0), COALESCE(l.longitude, 0.0),
                        c.measured_on, c.measured_time, c.temperature_c, c.humidity_pct,
                        c.source, c.source_ref, c.created_at
                 FROM climate_readings c
                 LEFT JOIN locations l ON l.id = 1
                 WHERE c.source = 'weather';
                 DELETE FROM climate_readings WHERE source = 'weather';

                 PRAGMA user_version = 13;
                 COMMIT;",
            )?;
        }

        if version < 14 {
            self.conn.execute_batch(
                "BEGIN;
                 CREATE TABLE planting_placements (
                     id          INTEGER PRIMARY KEY AUTOINCREMENT,
                     planting_id INTEGER NOT NULL REFERENCES plantings(id) ON DELETE CASCADE,
                     space_id    INTEGER NOT NULL REFERENCES spaces(id) ON DELETE RESTRICT,
                     started_on  TEXT NOT NULL,
                     ended_on    TEXT,
                     space_kind  TEXT NOT NULL,
                     light_type  TEXT,
                     light_hours REAL,
                     created_at  TEXT NOT NULL
                         DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                     CHECK (ended_on IS NULL OR ended_on >= started_on)
                 );
                 CREATE INDEX planting_placements_planting
                     ON planting_placements(planting_id, started_on);
                 CREATE INDEX planting_placements_space
                     ON planting_placements(space_id, started_on);
                 CREATE UNIQUE INDEX planting_placements_open
                     ON planting_placements(planting_id) WHERE ended_on IS NULL;

                 -- Every existing planting gets the place it has been in so far.
                 INSERT INTO planting_placements
                     (planting_id, space_id, started_on, ended_on, space_kind, light_type,
                      light_hours)
                 SELECT p.id, p.space_id, p.started_on,
                        CASE
                            WHEN p.ended_on IS NOT NULL AND p.ended_on >= p.started_on
                                THEN p.ended_on
                            ELSE NULL
                        END,
                        s.kind, s.light_type, s.light_hours
                 FROM plantings p
                 JOIN spaces s ON s.id = p.space_id;

                 PRAGMA user_version = 14;
                 COMMIT;",
            )?;
        }

        if version < 15 {
            // A development database can have the column while its version is still behind (for
            // example after an interrupted migration); adding it twice would fail.
            let has_repeats: bool = self.conn.query_row(
                "SELECT EXISTS(
                     SELECT 1 FROM pragma_table_info('profile_stages') WHERE name = 'repeats'
                 )",
                [],
                |row| row.get(0),
            )?;

            if has_repeats {
                self.conn.execute_batch("PRAGMA user_version = 15;")?;
            } else {
                self.conn.execute_batch(
                    "BEGIN;
                     ALTER TABLE profile_stages ADD COLUMN repeats INTEGER;
                     PRAGMA user_version = 15;
                     COMMIT;",
                )?;
            }
        }

        if version < 16 {
            // The time-axis graph needs the hourly series and the sun times. A powered light also
            // needs the minute it comes on; the placement keeps the value it had at the move.
            let has_column = |table: &str, column: &str| -> Result<bool> {
                Ok(self.conn.query_row(
                    "SELECT EXISTS(
                         SELECT 1 FROM pragma_table_info(?1) WHERE name = ?2
                     )",
                    [table, column],
                    |row| row.get(0),
                )?)
            };

            self.conn.execute_batch("BEGIN;")?;

            if !has_column("weather_daily", "sunrise")? {
                self.conn
                    .execute_batch("ALTER TABLE weather_daily ADD COLUMN sunrise TEXT;")?;
            }
            if !has_column("weather_daily", "sunset")? {
                self.conn
                    .execute_batch("ALTER TABLE weather_daily ADD COLUMN sunset TEXT;")?;
            }
            if !has_column("spaces", "light_start")? {
                self.conn
                    .execute_batch("ALTER TABLE spaces ADD COLUMN light_start INTEGER;")?;
            }
            if !has_column("planting_placements", "light_start")? {
                self.conn.execute_batch(
                    "ALTER TABLE planting_placements ADD COLUMN light_start INTEGER;",
                )?;
            }

            self.conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS weather_hours (
                     id              INTEGER PRIMARY KEY AUTOINCREMENT,
                     latitude        REAL NOT NULL,
                     longitude       REAL NOT NULL,
                     date            TEXT NOT NULL,
                     time            TEXT NOT NULL,
                     temperature_c   REAL,
                     humidity_pct    REAL,
                     precipitation_mm REAL,
                     weather_code    INTEGER,
                     cloud_cover_pct REAL,
                     sunshine_hours  REAL,
                     is_day          INTEGER,
                     source          TEXT NOT NULL DEFAULT 'weather',
                     source_ref      TEXT,
                     fetched_at      TEXT NOT NULL
                         DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                     CHECK (is_day IS NULL OR is_day IN (0, 1)),
                     UNIQUE(latitude, longitude, date, time)
                 );
                 PRAGMA user_version = 16;
                 COMMIT;",
            )?;
        }

        if version < 17 {
            self.conn.execute_batch(
                "BEGIN;
                 CREATE TABLE IF NOT EXISTS avatar_icons (
                     owner_type TEXT NOT NULL,
                     owner_id   INTEGER NOT NULL,
                     icon       TEXT NOT NULL,
                     updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                     PRIMARY KEY (owner_type, owner_id)
                 );
                 PRAGMA user_version = 17;
                 COMMIT;",
            )?;
        }

        Ok(())
    }

    fn seed_attribute_definitions(&self) -> Result<()> {
        let mut statement = self.conn.prepare(
            "INSERT INTO attribute_definitions
                 (key, label, description, value_type, unit, category, applies_to, enum_values,
                  is_custom)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'profile', ?7, 0)
             ON CONFLICT(key) DO UPDATE SET
                 label = excluded.label,
                 description = excluded.description,
                 value_type = excluded.value_type,
                 unit = excluded.unit,
                 category = excluded.category,
                 applies_to = excluded.applies_to,
                 enum_values = excluded.enum_values
             WHERE attribute_definitions.is_custom = 0
               AND (attribute_definitions.label IS NOT excluded.label
                    OR attribute_definitions.description IS NOT excluded.description
                    OR attribute_definitions.value_type IS NOT excluded.value_type
                    OR attribute_definitions.unit IS NOT excluded.unit
                    OR attribute_definitions.category IS NOT excluded.category
                    OR attribute_definitions.applies_to IS NOT excluded.applies_to
                    OR attribute_definitions.enum_values IS NOT excluded.enum_values)",
        )?;

        for seed in ATTRIBUTE_SEEDS {
            statement.execute(params![
                seed.key,
                seed.label,
                seed.description,
                seed.value_type,
                seed.unit,
                category_for(seed.key),
                seed.enum_values,
            ])?;
        }

        Ok(())
    }

    pub fn profiles(&self) -> Result<Vec<Profile>> {
        let mut statement = self.conn.prepare(
            "SELECT id, name, species, cultivar
             FROM profiles
             ORDER BY name COLLATE NOCASE, id",
        )?;

        let rows = statement.query_map([], |row| {
            Ok(Profile {
                id: row.get(0)?,
                name: row.get(1)?,
                species: row.get(2)?,
                cultivar: row.get(3)?,
            })
        })?;

        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn profile(&self, id: i64) -> Result<Option<Profile>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, name, species, cultivar FROM profiles WHERE id = ?1",
                [id],
                |row| {
                    Ok(Profile {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        species: row.get(2)?,
                        cultivar: row.get(3)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn location(&self) -> Result<Option<Location>> {
        Ok(self
            .conn
            .query_row(
                "SELECT latitude, longitude, name, region, country, timezone
                 FROM locations WHERE id = 1",
                [],
                |row| {
                    Ok(Location {
                        latitude: row.get(0)?,
                        longitude: row.get(1)?,
                        name: row.get(2)?,
                        region: row.get(3)?,
                        country: row.get(4)?,
                        timezone: row.get(5)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn set_location(&self, location: Location) -> Result<()> {
        self.conn.execute(
            "INSERT INTO locations
                 (id, latitude, longitude, name, region, country, timezone)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                 latitude = excluded.latitude,
                 longitude = excluded.longitude,
                 name = excluded.name,
                 region = excluded.region,
                 country = excluded.country,
                 timezone = excluded.timezone,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')",
            params![
                location.latitude,
                location.longitude,
                clean(location.name.as_deref()),
                clean(location.region.as_deref()),
                clean(location.country.as_deref()),
                clean(location.timezone.as_deref()),
            ],
        )?;

        Ok(())
    }

    pub fn clear_location(&self) -> Result<()> {
        self.conn
            .execute("DELETE FROM locations WHERE id = 1", [])?;
        Ok(())
    }

    // A space groups plantings that share an environment. A space without a name is private to
    // one planting, so the user never has to create a place before creating a planting.

    pub fn spaces(&self) -> Result<Vec<Space>> {
        let mut statement = self.conn.prepare(
            "SELECT id, name, kind, light_type, light_hours, light_start, notes
             FROM spaces
             WHERE name IS NOT NULL
             ORDER BY name COLLATE NOCASE, id",
        )?;

        let rows = statement.query_map([], space_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn space(&self, id: i64) -> Result<Option<Space>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, name, kind, light_type, light_hours, light_start, notes
                 FROM spaces WHERE id = ?1",
                [id],
                space_from_row,
            )
            .optional()?)
    }

    /// A named space, matched without case. `None` for an empty name.
    pub fn find_space(&self, name: &str) -> Result<Option<Space>> {
        let name = name.trim();
        if name.is_empty() {
            return Ok(None);
        }

        Ok(self
            .conn
            .query_row(
                "SELECT id, name, kind, light_type, light_hours, light_start, notes
                 FROM spaces WHERE name = ?1 COLLATE NOCASE",
                [name],
                space_from_row,
            )
            .optional()?)
    }

    pub fn create_space(&self, input: &SpaceInput) -> Result<i64> {
        input.validate()?;
        let name = clean(input.name.as_deref());
        if let Some(name) = name
            && self.find_space(name)?.is_some()
        {
            return Err(ValidationError(format!("a space named {name:?} already exists")).into());
        }

        self.conn.execute(
            "INSERT INTO spaces (name, kind, light_type, light_hours, light_start, notes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                name,
                input.kind,
                input.light_type,
                input.light_hours,
                input.light_start,
                clean(input.notes.as_deref()),
            ],
        )?;

        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_space(&self, id: i64, input: &SpaceInput) -> Result<()> {
        input.validate()?;
        let current = self
            .space(id)?
            .ok_or_else(|| ValidationError(format!("space {id} does not exist")))?;

        if current.kind != input.kind && self.space_has_history(id)? {
            return Err(ValidationError(
                "a space with plantings, placements or readings keeps its type; create a new space instead"
                    .into(),
            )
            .into());
        }

        let name = clean(input.name.as_deref());
        // Only removing a name is a change: an already-private space must stay editable even
        // when history from an earlier planting lives there.
        let losing_name = current.name.is_some() && name.is_none();
        if losing_name && self.space_planting_count(id, None)? > 1 {
            return Err(ValidationError("a shared space needs a name".into()).into());
        }
        if losing_name && self.space_other_placement_count(id)? > 0 {
            return Err(ValidationError(
                "a space with placement history from another planting needs a name".into(),
            )
            .into());
        }

        if let Some(name) = name
            && let Some(existing) = self
                .conn
                .query_row(
                    "SELECT id FROM spaces WHERE name = ?1 COLLATE NOCASE",
                    [name],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?
            && existing != id
        {
            return Err(ValidationError(format!("a space named {name:?} already exists")).into());
        }

        self.atomically(|conn| {
            conn.execute(
                "UPDATE spaces SET
                     name = ?1,
                     kind = ?2,
                     light_type = ?3,
                     light_hours = ?4,
                     light_start = ?5,
                     notes = ?6,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
                 WHERE id = ?7",
                params![
                    name,
                    input.kind,
                    input.light_type,
                    input.light_hours,
                    input.light_start,
                    clean(input.notes.as_deref()),
                    id,
                ],
            )?;

            // A change to the shared light reaches the plantings that are there now. A closed
            // placement keeps the light it had; that is the point of the snapshot.
            conn.execute(
                "UPDATE planting_placements
                 SET light_type = ?1, light_hours = ?2, light_start = ?3
                 WHERE space_id = ?4 AND ended_on IS NULL",
                params![input.light_type, input.light_hours, input.light_start, id,],
            )?;
            Ok(())
        })?;

        Ok(())
    }

    /// Placements in a space left by a planting that is no longer (only) there.
    fn space_other_placement_count(&self, id: i64) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*)
             FROM planting_placements p
             WHERE p.space_id = ?1
               AND p.planting_id NOT IN (SELECT id FROM plantings WHERE space_id = ?1)",
            [id],
            |row| row.get(0),
        )?)
    }

    /// How many plantings are in a space, optionally ignoring one row.
    fn space_planting_count(&self, id: i64, except: Option<i64>) -> Result<i64> {
        let count = match except {
            Some(except) => self.conn.query_row(
                "SELECT COUNT(*) FROM plantings WHERE space_id = ?1 AND id <> ?2",
                [id, except],
                |row| row.get(0),
            )?,
            None => self.conn.query_row(
                "SELECT COUNT(*) FROM plantings WHERE space_id = ?1",
                [id],
                |row| row.get(0),
            )?,
        };

        Ok(count)
    }

    pub fn plantings(&self) -> Result<Vec<PlantingSummary>> {
        let sql = format!(
            "{PLANTING_SELECT}
             ORDER BY (p.ended_on IS NOT NULL), LOWER(COALESCE(p.name, pr.name)), p.id"
        );
        let mut statement = self.conn.prepare(&sql)?;
        let rows = statement.query_map([], planting_summary_from_row)?;

        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn planting(&self, id: i64) -> Result<Option<PlantingSummary>> {
        let sql = format!("{PLANTING_SELECT} WHERE p.id = ?1");

        Ok(self
            .conn
            .query_row(&sql, [id], planting_summary_from_row)
            .optional()?)
    }

    pub fn create_planting(&self, input: &PlantingInput) -> Result<i64> {
        input.validate()?;
        if self.profile(input.profile_id)?.is_none() {
            return Err(
                ValidationError(format!("profile {} does not exist", input.profile_id)).into(),
            );
        }
        let space = self
            .space(input.space_id)?
            .ok_or_else(|| ValidationError(format!("space {} does not exist", input.space_id)))?;
        if space.display_name().is_none() && self.space_planting_count(space.id, None)? > 0 {
            return Err(ValidationError(
                "a private space holds one planting; give it a name first".into(),
            )
            .into());
        }

        let mut id = 0;
        self.atomically(|conn| {
            conn.execute(
                "INSERT INTO plantings
                     (profile_id, space_id, name, quantity, container, container_volume_l,
                      substrate, started_on, start_kind, notes)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    input.profile_id,
                    input.space_id,
                    clean(input.name.as_deref()),
                    input.quantity,
                    input.container,
                    input.container_volume_l,
                    clean(input.substrate.as_deref()),
                    input.started_on.to_iso(),
                    input.start_kind,
                    clean(input.notes.as_deref()),
                ],
            )?;

            id = conn.last_insert_rowid();
            self.insert_placement(id, &space, input.started_on, None)?;

            Ok(())
        })?;

        Ok(id)
    }

    pub fn update_planting(&self, id: i64, input: &PlantingInput) -> Result<()> {
        input.validate()?;

        let current = self
            .planting(id)?
            .ok_or_else(|| ValidationError(format!("planting {id} does not exist")))?;

        if current.planting.profile_id != input.profile_id {
            return Err(
                ValidationError("a planting cannot change its plant profile".into()).into(),
            );
        }
        let target = self
            .space(input.space_id)?
            .ok_or_else(|| ValidationError(format!("space {} does not exist", input.space_id)))?;
        if target.display_name().is_none()
            && target.id != current.planting.space_id
            && self.space_planting_count(target.id, Some(id))? > 0
        {
            return Err(ValidationError(
                "a private space holds one planting; give it a name first".into(),
            )
            .into());
        }

        let changing_space = current.planting.space_id != input.space_id;
        if changing_space && current.planting.ended_on.is_some() {
            return Err(ValidationError(
                "reopen the planting before moving it to another space".into(),
            )
            .into());
        }

        let changing_start = current.planting.started_on != input.started_on;
        if changing_start
            && current
                .planting
                .ended_on
                .is_some_and(|ended| input.started_on > ended)
        {
            return Err(ValidationError(
                "the start date cannot be after the planting's end date".into(),
            )
            .into());
        }
        if changing_start && changing_space {
            return Err(ValidationError(
                "change the start date and the space in separate saves".into(),
            )
            .into());
        }
        if changing_start {
            let placements = self.planting_placements(id)?;
            if let Some(first) = placements.first()
                && let Some(ended) = first.ended_on
                && input.started_on > ended
            {
                return Err(ValidationError(
                    "the start date cannot be after the planting's first move".into(),
                )
                .into());
            }
        }

        let today = crate::clock::today();
        let moved_on = if changing_space {
            Some(input.moved_on.unwrap_or_else(|| {
                if current.planting.started_on > today {
                    current.planting.started_on
                } else {
                    today
                }
            }))
        } else {
            None
        };

        // The UI sends the local date, and `clock::today` reads the same clock.
        if let Some(moved_on) = moved_on
            && current.planting.started_on <= today
            && moved_on > today
        {
            return Err(ValidationError("a move cannot be in the future".into()).into());
        }

        self.atomically(|_| {
            self.conn.execute(
                "UPDATE plantings SET
                     space_id = ?1,
                     name = ?2,
                     quantity = ?3,
                     container = ?4,
                     container_volume_l = ?5,
                     substrate = ?6,
                     started_on = ?7,
                     start_kind = ?8,
                     notes = ?9,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
                 WHERE id = ?10",
                params![
                    input.space_id,
                    clean(input.name.as_deref()),
                    input.quantity,
                    input.container,
                    input.container_volume_l,
                    clean(input.substrate.as_deref()),
                    input.started_on.to_iso(),
                    input.start_kind,
                    clean(input.notes.as_deref()),
                    id,
                ],
            )?;

            if changing_start {
                let first: i64 = self.conn.query_row(
                    "SELECT id FROM planting_placements
                     WHERE planting_id = ?1
                     ORDER BY started_on ASC, id ASC
                     LIMIT 1",
                    [id],
                    |row| row.get(0),
                )?;
                self.conn.execute(
                    "UPDATE planting_placements SET started_on = ?1 WHERE id = ?2",
                    params![input.started_on.to_iso(), first],
                )?;
            }

            if let Some(moved_on) = moved_on {
                let open = self.open_placement(id)?.ok_or_else(|| {
                    ValidationError(format!("planting {id} has no open placement"))
                })?;

                if moved_on < open.started_on {
                    return Err(ValidationError(
                        "a move cannot be before the current placement started".into(),
                    )
                    .into());
                }

                if moved_on <= open.started_on {
                    self.conn.execute(
                        "UPDATE planting_placements SET
                             space_id = ?1,
                             space_kind = ?2,
                             light_type = ?3,
                             light_hours = ?4,
                             light_start = ?5
                         WHERE id = ?6",
                        params![
                            input.space_id,
                            target.kind,
                            target.light_type,
                            target.light_hours,
                            target.light_start,
                            open.id,
                        ],
                    )?;
                } else {
                    self.conn.execute(
                        "UPDATE planting_placements SET ended_on = ?1 WHERE id = ?2",
                        params![moved_on.add_days(-1).to_iso(), open.id],
                    )?;
                    self.insert_placement(id, &target, moved_on, None)?;
                }
            }

            Ok(())
        })?;

        Ok(())
    }

    pub fn finish_planting(
        &self,
        id: i64,
        ended_on: crate::calendar::Date,
        outcome: &str,
    ) -> Result<()> {
        validate_outcome(outcome)?;

        let current = self
            .planting(id)?
            .ok_or_else(|| ValidationError(format!("planting {id} does not exist")))?;

        if ended_on < current.planting.started_on {
            return Err(
                ValidationError("the end date cannot be before the start date".into()).into(),
            );
        }

        if let Some(open) = self.open_placement(id)?
            && ended_on < open.started_on
        {
            return Err(ValidationError(
                "the end date cannot be before the current placement started".into(),
            )
            .into());
        }

        self.atomically(|_| {
            self.conn.execute(
                "UPDATE plantings SET
                     ended_on = ?1,
                     outcome = ?2,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
                 WHERE id = ?3",
                params![ended_on.to_iso(), outcome, id],
            )?;
            self.conn.execute(
                "UPDATE planting_placements SET ended_on = ?1
                 WHERE planting_id = ?2 AND ended_on IS NULL",
                params![ended_on.to_iso(), id],
            )?;
            Ok(())
        })?;

        Ok(())
    }

    pub fn reopen_planting(&self, id: i64) -> Result<()> {
        if self.planting(id)?.is_none() {
            return Err(ValidationError(format!("planting {id} does not exist")).into());
        }

        self.atomically(|conn| {
            conn.execute(
                "UPDATE plantings SET
                     ended_on = NULL,
                     outcome = NULL,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
                 WHERE id = ?1",
                [id],
            )?;
            conn.execute(
                "UPDATE planting_placements SET ended_on = NULL
                 WHERE id = (
                     SELECT id FROM planting_placements
                     WHERE planting_id = ?1
                     ORDER BY started_on DESC, id DESC
                     LIMIT 1
                 )",
                [id],
            )?;
            Ok(())
        })
    }

    /// Deletes a planting and returns the image file stems it removed, including those of any
    /// private space that went with it. The caller removes the files after the transaction.
    pub fn delete_planting(&self, id: i64) -> Result<Vec<String>> {
        let Some(_current) = self.planting(id)? else {
            return Ok(Vec::new());
        };

        // Private spaces that only this planting used should not linger hidden after the
        // planting is gone. Their readings cascade with the space.
        let private_spaces: Vec<i64> = {
            let mut statement = self.conn.prepare(
                "SELECT DISTINCT pl.space_id
                 FROM planting_placements pl
                 JOIN spaces s ON s.id = pl.space_id
                 WHERE pl.planting_id = ?1 AND s.name IS NULL",
            )?;
            let rows = statement.query_map([id], |row| row.get(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };

        let transaction = self.conn.unchecked_transaction()?;
        let mut stems: Vec<String> = {
            let mut statement = transaction.prepare(
                "SELECT file_stem FROM media
                 WHERE (owner_type = 'observation'
                        AND owner_id IN (SELECT id FROM observations WHERE planting_id = ?1))
                    OR (owner_type = 'planting' AND owner_id = ?1)
                 ORDER BY id",
            )?;
            let rows = statement.query_map([id], |row| row.get(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };

        transaction.execute(
            "DELETE FROM media
             WHERE owner_type = 'observation'
               AND owner_id IN (SELECT id FROM observations WHERE planting_id = ?1)",
            [id],
        )?;
        transaction.execute(
            "DELETE FROM media WHERE owner_type = 'planting' AND owner_id = ?1",
            [id],
        )?;
        transaction.execute(
            "DELETE FROM avatar_icons WHERE owner_type = 'planting' AND owner_id = ?1",
            [id],
        )?;
        transaction.execute("DELETE FROM plantings WHERE id = ?1", [id])?;
        for space in private_spaces {
            // Another planting can still have a closed placement here after a move. Keep the
            // space and its rows rather than failing the foreign key and leaving the planting
            // undeletable.
            let deleted = transaction.execute(
                "DELETE FROM spaces
                 WHERE id = ?1 AND name IS NULL
                   AND NOT EXISTS (
                       SELECT 1 FROM planting_placements p
                       WHERE p.space_id = spaces.id AND p.planting_id <> ?2
                   )",
                params![space, id],
            )?;
            if deleted > 0 {
                let mut statement = transaction.prepare(
                    "SELECT file_stem FROM media
                     WHERE owner_type = 'space' AND owner_id = ?1
                     ORDER BY id",
                )?;
                let rows = statement.query_map([space], |row| row.get(0))?;
                stems.extend(rows.collect::<rusqlite::Result<Vec<String>>>()?);
                drop(statement);

                transaction.execute(
                    "DELETE FROM media WHERE owner_type = 'space' AND owner_id = ?1",
                    [space],
                )?;
                transaction.execute(
                    "DELETE FROM avatar_icons WHERE owner_type = 'space' AND owner_id = ?1",
                    [space],
                )?;
            }
        }
        transaction.commit()?;

        Ok(stems)
    }

    // Observations are the planting's diary. A repot changes the current pot volume and keeps
    // the previous one on the entry.

    fn insert_placement(
        &self,
        planting_id: i64,
        space: &Space,
        started_on: crate::calendar::Date,
        ended_on: Option<crate::calendar::Date>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO planting_placements
                 (planting_id, space_id, started_on, ended_on, space_kind, light_type,
                  light_hours, light_start)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                planting_id,
                space.id,
                started_on.to_iso(),
                ended_on.map(|date| date.to_iso()),
                space.kind,
                space.light_type,
                space.light_hours,
                space.light_start,
            ],
        )?;

        Ok(())
    }

    pub fn planting_placements(&self, planting_id: i64) -> Result<Vec<PlantingPlacement>> {
        let mut statement = self.conn.prepare(
            "SELECT id, planting_id, space_id, started_on, ended_on, space_kind, light_type,
                    light_hours, light_start
             FROM planting_placements
             WHERE planting_id = ?1
             ORDER BY started_on ASC, id ASC",
        )?;

        let rows = statement.query_map([planting_id], placement_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    fn open_placement(&self, planting_id: i64) -> Result<Option<PlantingPlacement>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, planting_id, space_id, started_on, ended_on, space_kind, light_type,
                        light_hours, light_start
                 FROM planting_placements
                 WHERE planting_id = ?1 AND ended_on IS NULL
                 ORDER BY started_on DESC, id DESC
                 LIMIT 1",
                [planting_id],
                placement_from_row,
            )
            .optional()?)
    }

    /// Local readings from every space the planting occupied, within that placement.
    pub fn planting_climate_readings(&self, planting_id: i64) -> Result<Vec<ClimateReading>> {
        let mut statement = self.conn.prepare(
            "SELECT c.id, c.space_id, c.measured_on, c.measured_time, c.temperature_c,
                    c.humidity_pct, c.source, c.source_ref
             FROM climate_readings c
             JOIN planting_placements pl ON pl.space_id = c.space_id
             WHERE pl.planting_id = ?1
               AND c.measured_on >= pl.started_on
               AND (pl.ended_on IS NULL OR c.measured_on <= pl.ended_on)
             GROUP BY c.id
             ORDER BY c.measured_on DESC, c.measured_time DESC, c.id DESC",
        )?;

        let rows = statement.query_map([planting_id], climate_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Weather-model days for the outdoor placements of a planting at the current location.
    pub fn planting_weather_days(&self, planting_id: i64) -> Result<Vec<WeatherDay>> {
        let Some(location) = self.location()? else {
            return Ok(Vec::new());
        };

        let mut statement = self.conn.prepare(
            "SELECT wd.latitude, wd.longitude, wd.date, wd.temp_min_c, wd.temp_max_c,
                    wd.temp_mean_c, wd.humidity_mean_pct, wd.humidity_min_pct,
                    wd.humidity_max_pct, wd.precipitation_mm, wd.precipitation_hours,
                    wd.weather_code, wd.sunshine_hours, wd.daylight_hours,
                    wd.radiation_sum_mj_m2, wd.wind_max_kmh, wd.wind_gust_max_kmh, wd.et0_mm,
                    wd.sunrise, wd.sunset, wd.night_start, wd.night_end, wd.night_temp_avg_c,
                    wd.night_temp_min_c, wd.night_humidity_avg_pct, wd.night_hours
             FROM weather_daily wd
             JOIN planting_placements pl ON pl.space_kind = 'outdoor'
             WHERE pl.planting_id = ?1
               AND wd.latitude = ?2 AND wd.longitude = ?3
               AND wd.date >= pl.started_on
               AND (pl.ended_on IS NULL OR wd.date <= pl.ended_on)
             GROUP BY wd.id
             ORDER BY wd.date ASC",
        )?;

        let rows = statement.query_map(
            params![planting_id, location.latitude, location.longitude],
            weather_day_from_row,
        )?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Whether a space has plantings, placements or readings that a type change would reinterpret.
    pub fn space_has_history(&self, space_id: i64) -> Result<bool> {
        let exists: i64 = self.conn.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM plantings WHERE space_id = ?1
                 UNION ALL
                 SELECT 1 FROM planting_placements WHERE space_id = ?1
                 UNION ALL
                 SELECT 1 FROM climate_readings WHERE space_id = ?1
             )",
            [space_id],
            |row| row.get(0),
        )?;

        Ok(exists != 0)
    }

    pub fn observations(&self, planting_id: i64) -> Result<Vec<Observation>> {
        let mut statement = self.conn.prepare(
            "SELECT id, planting_id, kind, occurred_on, occurred_time, note, amount, unit,
                    stage_id, previous_amount, previous_unit
             FROM observations
             WHERE planting_id = ?1
             ORDER BY occurred_on DESC, occurred_time DESC, id DESC",
        )?;

        let rows = statement.query_map([planting_id], observation_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn observation(&self, id: i64) -> Result<Option<Observation>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, planting_id, kind, occurred_on, occurred_time, note, amount, unit,
                        stage_id, previous_amount, previous_unit
                 FROM observations WHERE id = ?1",
                [id],
                observation_from_row,
            )
            .optional()?)
    }

    /// Rewrites the repot chain in time order and returns the last known pot size.
    ///
    /// Entries without a time sort before timed entries of the same day, matching the diary.
    fn rebuild_repot_chain(
        conn: &Connection,
        planting_id: i64,
        initial: Option<f64>,
    ) -> Result<Option<f64>> {
        let mut statement = conn.prepare(
            "SELECT id, occurred_on, occurred_time, amount
             FROM observations
             WHERE planting_id = ?1 AND kind = 'repot'",
        )?;
        let rows = statement.query_map([planting_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<f64>>(3)?,
            ))
        })?;
        let mut entries = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        entries.sort_by(|a, b| {
            a.1.cmp(&b.1)
                .then_with(|| match (&a.2, &b.2) {
                    (None, None) => std::cmp::Ordering::Equal,
                    (None, Some(_)) => std::cmp::Ordering::Less,
                    (Some(_), None) => std::cmp::Ordering::Greater,
                    (Some(a), Some(b)) => a.cmp(b),
                })
                .then_with(|| a.0.cmp(&b.0))
        });

        let mut last = initial;
        for (id, _, _, amount) in entries {
            conn.execute(
                "UPDATE observations
                 SET previous_amount = ?1, previous_unit = ?2
                 WHERE id = ?3",
                params![last, last.map(|_| "L"), id],
            )?;
            if let Some(amount) = amount {
                last = Some(amount);
            }
        }

        Ok(last)
    }

    pub fn create_observation(&self, input: &ObservationInput) -> Result<i64> {
        input.validate()?;

        let planting = self.planting(input.planting_id)?.ok_or_else(|| {
            ValidationError(format!("planting {} does not exist", input.planting_id))
        })?;

        if input.occurred_on < planting.planting.started_on {
            return Err(ValidationError(
                "a diary entry cannot be before the planting started".into(),
            )
            .into());
        }

        if input.kind == "stage" {
            let stage_id = input.stage_id.expect("validated");
            let belongs: bool = self.conn.query_row(
                "SELECT EXISTS(
                     SELECT 1 FROM profile_stages
                     WHERE id = ?1 AND profile_id = ?2
                 )",
                params![stage_id, planting.planting.profile_id],
                |row| row.get(0),
            )?;
            if !belongs {
                return Err(ValidationError(
                    "that stage does not belong to this plant profile".into(),
                )
                .into());
            }
        }

        if input.kind == "repot" && planting.planting.container != "pot" {
            return Err(ValidationError("only a potted planting can be repotted".into()).into());
        }

        let time = input
            .occurred_time
            .as_deref()
            .and_then(|text| parse_time(text).ok().flatten());

        // The size before the first repot: its own "before" when the chain already exists,
        // otherwise the pot size the planting starts with.
        let initial_volume = if input.kind == "repot" {
            let earliest: Option<Option<f64>> = self
                .conn
                .query_row(
                    "SELECT previous_amount FROM observations
                     WHERE planting_id = ?1 AND kind = 'repot'
                     ORDER BY occurred_on ASC, occurred_time ASC, id ASC
                     LIMIT 1",
                    [input.planting_id],
                    |row| row.get(0),
                )
                .optional()?;
            match earliest {
                Some(previous) => previous,
                None => planting.planting.container_volume_l,
            }
        } else {
            None
        };

        let transaction = self.conn.unchecked_transaction()?;
        transaction.execute(
            "INSERT INTO observations
                 (planting_id, kind, occurred_on, occurred_time, note, amount, unit, stage_id,
                  previous_amount, previous_unit)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, NULL)",
            params![
                input.planting_id,
                input.kind.as_str(),
                input.occurred_on.to_iso(),
                time,
                clean(input.note.as_deref()),
                input.amount,
                input.unit.as_deref(),
                input.stage_id,
            ],
        )?;
        let id = transaction.last_insert_rowid();

        if input.kind == "repot" {
            let last = Self::rebuild_repot_chain(&transaction, input.planting_id, initial_volume)?;
            // An entry that records no new size does not touch the pot size; a size typed in the
            // planting form or kept by a delete stays.
            if input.amount.is_some()
                && let Some(last) = last
            {
                transaction.execute(
                    "UPDATE plantings SET
                         container_volume_l = ?1,
                         updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
                     WHERE id = ?2",
                    params![last, input.planting_id],
                )?;
            }
        }

        transaction.commit()?;
        Ok(id)
    }

    /// Deletes an entry and its media rows; the caller removes the returned files.
    pub fn delete_observation(&self, id: i64) -> Result<Vec<String>> {
        let mut statement = self.conn.prepare(
            "SELECT file_stem FROM media
             WHERE owner_type = 'observation' AND owner_id = ?1",
        )?;
        let stems: Vec<String> = statement
            .query_map([id], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(statement);

        let transaction = self.conn.unchecked_transaction()?;
        let repot = self
            .observation(id)?
            .filter(|observation| observation.kind == "repot");
        let initial = if let Some(observation) = &repot {
            transaction
                .query_row(
                    "SELECT previous_amount FROM observations
                     WHERE planting_id = ?1 AND kind = 'repot'
                     ORDER BY occurred_on ASC, occurred_time ASC, id ASC
                     LIMIT 1",
                    [observation.planting_id],
                    |row| row.get::<_, Option<f64>>(0),
                )
                .optional()?
                .flatten()
        } else {
            None
        };
        transaction.execute(
            "DELETE FROM media WHERE owner_type = 'observation' AND owner_id = ?1",
            [id],
        )?;
        transaction.execute("DELETE FROM observations WHERE id = ?1", [id])?;
        if let Some(observation) = repot {
            Self::rebuild_repot_chain(&transaction, observation.planting_id, initial)?;
        }
        transaction.commit()?;

        Ok(stems)
    }

    /// Record one weather-model snapshot. The unique index makes a repeated sync harmless.
    pub fn insert_weather_current(&self, current: &WeatherCurrent) -> Result<bool> {
        let changed = self.conn.execute(
            "INSERT OR IGNORE INTO weather_current
                 (latitude, longitude, observed_on, observed_time, temperature_c, humidity_pct,
                  apparent_temperature_c, weather_code, precipitation_mm, cloud_cover_pct,
                  wind_speed_kmh, wind_gust_kmh, is_day, source, source_ref)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 'weather', ?14)",
            params![
                current.latitude,
                current.longitude,
                current.observed_on.to_iso(),
                current.observed_time,
                current.temperature_c,
                current.humidity_pct,
                current.apparent_temperature_c,
                current.weather_code,
                current.precipitation_mm,
                current.cloud_cover_pct,
                current.wind_speed_kmh,
                current.wind_gust_kmh,
                current.is_day.map(|is_day| if is_day { 1 } else { 0 }),
                crate::weather::PROVIDER,
            ],
        )?;

        Ok(changed > 0)
    }

    /// The newest model snapshot for this location.
    pub fn weather_current_latest(
        &self,
        latitude: f64,
        longitude: f64,
    ) -> Result<Option<WeatherCurrent>> {
        let current = self
            .conn
            .query_row(
                "SELECT latitude, longitude, observed_on, observed_time, temperature_c,
                        humidity_pct, apparent_temperature_c, weather_code, precipitation_mm,
                        cloud_cover_pct, wind_speed_kmh, wind_gust_kmh, is_day
                 FROM weather_current
                 WHERE latitude = ?1 AND longitude = ?2
                 ORDER BY observed_on DESC, observed_time DESC, id DESC
                 LIMIT 1",
                params![latitude, longitude],
                weather_current_from_row,
            )
            .optional()?;

        Ok(current)
    }

    /// Insert or update one daily weather row. The latest sync wins; yesterday's night is stable.
    pub fn upsert_weather_day(&self, day: &WeatherDay) -> Result<()> {
        self.conn.execute(
            "INSERT INTO weather_daily
                 (latitude, longitude, date, temp_min_c, temp_max_c, temp_mean_c,
                  humidity_mean_pct, humidity_min_pct, humidity_max_pct, precipitation_mm,
                  precipitation_hours, weather_code, sunshine_hours, daylight_hours,
                  radiation_sum_mj_m2, wind_max_kmh, wind_gust_max_kmh, et0_mm, sunrise, sunset,
                  night_start, night_end, night_temp_avg_c, night_temp_min_c,
                  night_humidity_avg_pct, night_hours, source, source_ref)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                     ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, 'weather', ?27)
             ON CONFLICT(latitude, longitude, date) DO UPDATE SET
                 -- A partial provider answer keeps what is already stored for that day.
                 temp_min_c = COALESCE(excluded.temp_min_c, weather_daily.temp_min_c),
                 temp_max_c = COALESCE(excluded.temp_max_c, weather_daily.temp_max_c),
                 temp_mean_c = COALESCE(excluded.temp_mean_c, weather_daily.temp_mean_c),
                 humidity_mean_pct = COALESCE(excluded.humidity_mean_pct, weather_daily.humidity_mean_pct),
                 humidity_min_pct = COALESCE(excluded.humidity_min_pct, weather_daily.humidity_min_pct),
                 humidity_max_pct = COALESCE(excluded.humidity_max_pct, weather_daily.humidity_max_pct),
                 precipitation_mm = COALESCE(excluded.precipitation_mm, weather_daily.precipitation_mm),
                 precipitation_hours = COALESCE(excluded.precipitation_hours, weather_daily.precipitation_hours),
                 weather_code = COALESCE(excluded.weather_code, weather_daily.weather_code),
                 sunshine_hours = COALESCE(excluded.sunshine_hours, weather_daily.sunshine_hours),
                 daylight_hours = COALESCE(excluded.daylight_hours, weather_daily.daylight_hours),
                 radiation_sum_mj_m2 = COALESCE(excluded.radiation_sum_mj_m2, weather_daily.radiation_sum_mj_m2),
                 wind_max_kmh = COALESCE(excluded.wind_max_kmh, weather_daily.wind_max_kmh),
                 wind_gust_max_kmh = COALESCE(excluded.wind_gust_max_kmh, weather_daily.wind_gust_max_kmh),
                 et0_mm = COALESCE(excluded.et0_mm, weather_daily.et0_mm),
                 sunrise = COALESCE(excluded.sunrise, weather_daily.sunrise),
                 sunset = COALESCE(excluded.sunset, weather_daily.sunset),
                 night_start = CASE WHEN ?28 THEN excluded.night_start ELSE weather_daily.night_start END,
                 night_end = CASE WHEN ?28 THEN excluded.night_end ELSE weather_daily.night_end END,
                 night_temp_avg_c = CASE WHEN ?28 THEN excluded.night_temp_avg_c ELSE weather_daily.night_temp_avg_c END,
                 night_temp_min_c = CASE WHEN ?28 THEN excluded.night_temp_min_c ELSE weather_daily.night_temp_min_c END,
                 night_humidity_avg_pct = CASE WHEN ?28 THEN excluded.night_humidity_avg_pct ELSE weather_daily.night_humidity_avg_pct END,
                 night_hours = CASE WHEN ?28 THEN excluded.night_hours ELSE weather_daily.night_hours END,
                 source = excluded.source,
                 source_ref = excluded.source_ref,
                 fetched_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')",
            params![
                day.latitude,
                day.longitude,
                day.date.to_iso(),
                day.temp_min_c,
                day.temp_max_c,
                day.temp_mean_c,
                day.humidity_mean_pct,
                day.humidity_min_pct,
                day.humidity_max_pct,
                day.precipitation_mm,
                day.precipitation_hours,
                day.weather_code,
                day.sunshine_hours,
                day.daylight_hours,
                day.radiation_sum_mj_m2,
                day.wind_max_kmh,
                day.wind_gust_max_kmh,
                day.et0_mm,
                day.sunrise,
                day.sunset,
                day.night_start,
                day.night_end,
                day.night_temp_avg_c,
                day.night_temp_min_c,
                day.night_humidity_avg_pct,
                day.night_hours,
                crate::weather::PROVIDER,
                if day.night_complete { 1 } else { 0 },
            ],
        )?;

        Ok(())
    }

    /// Daily weather rows for this location, oldest first.
    pub fn weather_days(
        &self,
        latitude: f64,
        longitude: f64,
        since: crate::calendar::Date,
    ) -> Result<Vec<WeatherDay>> {
        let mut statement = self.conn.prepare(
            "SELECT latitude, longitude, date, temp_min_c, temp_max_c, temp_mean_c,
                    humidity_mean_pct, humidity_min_pct, humidity_max_pct, precipitation_mm,
                    precipitation_hours, weather_code, sunshine_hours, daylight_hours,
                    radiation_sum_mj_m2, wind_max_kmh, wind_gust_max_kmh, et0_mm, sunrise, sunset,
                    night_start, night_end, night_temp_avg_c, night_temp_min_c,
                    night_humidity_avg_pct, night_hours
             FROM weather_daily
             WHERE latitude = ?1 AND longitude = ?2 AND date >= ?3
             ORDER BY date ASC",
        )?;

        let rows = statement.query_map(
            params![latitude, longitude, since.to_iso()],
            weather_day_from_row,
        )?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Insert or update one hourly weather row.
    pub fn upsert_weather_hour(&self, hour: &WeatherHour) -> Result<()> {
        self.conn.execute(
            "INSERT INTO weather_hours
                 (latitude, longitude, date, time, temperature_c, humidity_pct,
                  precipitation_mm, weather_code, cloud_cover_pct, sunshine_hours, is_day,
                  source, source_ref)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'weather', ?12)
             ON CONFLICT(latitude, longitude, date, time) DO UPDATE SET
                 temperature_c = COALESCE(excluded.temperature_c, weather_hours.temperature_c),
                 humidity_pct = COALESCE(excluded.humidity_pct, weather_hours.humidity_pct),
                 precipitation_mm = COALESCE(excluded.precipitation_mm, weather_hours.precipitation_mm),
                 weather_code = COALESCE(excluded.weather_code, weather_hours.weather_code),
                 cloud_cover_pct = COALESCE(excluded.cloud_cover_pct, weather_hours.cloud_cover_pct),
                 sunshine_hours = COALESCE(excluded.sunshine_hours, weather_hours.sunshine_hours),
                 is_day = COALESCE(excluded.is_day, weather_hours.is_day),
                 source = excluded.source,
                 source_ref = excluded.source_ref,
                 fetched_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')",
            params![
                hour.latitude,
                hour.longitude,
                hour.date.to_iso(),
                hour.time,
                hour.temperature_c,
                hour.humidity_pct,
                hour.precipitation_mm,
                hour.weather_code,
                hour.cloud_cover_pct,
                hour.sunshine_hours,
                hour.is_day.map(|value| if value { 1 } else { 0 }),
                crate::weather::PROVIDER,
            ],
        )?;

        Ok(())
    }

    /// Hourly weather rows for this location in an inclusive date range, oldest first.
    pub fn weather_hours(
        &self,
        latitude: f64,
        longitude: f64,
        since: crate::calendar::Date,
        until: crate::calendar::Date,
    ) -> Result<Vec<WeatherHour>> {
        let mut statement = self.conn.prepare(
            "SELECT latitude, longitude, date, time, temperature_c, humidity_pct,
                    precipitation_mm, weather_code, cloud_cover_pct, sunshine_hours, is_day
             FROM weather_hours
             WHERE latitude = ?1 AND longitude = ?2 AND date >= ?3 AND date <= ?4
             ORDER BY date ASC, time ASC",
        )?;

        let rows = statement.query_map(
            params![latitude, longitude, since.to_iso(), until.to_iso()],
            weather_hour_from_row,
        )?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// The readings of one space, newest first.
    pub fn climate_readings(&self, space_id: i64) -> Result<Vec<ClimateReading>> {
        let mut statement = self.conn.prepare(
            "SELECT id, space_id, measured_on, measured_time, temperature_c, humidity_pct,
                    source, source_ref
             FROM climate_readings
             WHERE space_id = ?1
             ORDER BY measured_on DESC, measured_time DESC, id DESC",
        )?;

        let rows = statement.query_map([space_id], climate_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn create_climate_reading(&self, input: &ClimateInput) -> Result<i64> {
        input.validate()?;

        if self.space(input.space_id)?.is_none() {
            return Err(ValidationError(format!("space {} does not exist", input.space_id)).into());
        }

        if input.measured_on > crate::clock::today() {
            return Err(ValidationError("a reading cannot be in the future".into()).into());
        }

        self.conn.execute(
            "INSERT INTO climate_readings
                 (space_id, measured_on, measured_time, temperature_c, humidity_pct, source,
                  source_ref)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                input.space_id,
                input.measured_on.to_iso(),
                input
                    .measured_time
                    .as_deref()
                    .and_then(|text| parse_time(text).ok().flatten()),
                input.temperature_c,
                input.humidity_pct,
                input.source,
                clean(input.source_ref.as_deref()),
            ],
        )?;

        Ok(self.conn.last_insert_rowid())
    }

    pub fn delete_climate_reading(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM climate_readings WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Removes a just-created entry and undoes a repot's pot-volume change. The normal delete
    /// keeps the current volume; this is only for a save that failed after the entry was written.
    pub fn discard_observation(&self, id: i64) -> Result<Vec<String>> {
        let Some(observation) = self.observation(id)? else {
            return Ok(Vec::new());
        };

        let mut statement = self.conn.prepare(
            "SELECT file_stem FROM media
             WHERE owner_type = 'observation' AND owner_id = ?1",
        )?;
        let stems: Vec<String> = statement
            .query_map([id], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(statement);

        let transaction = self.conn.unchecked_transaction()?;
        let initial = if observation.kind == "repot" {
            transaction
                .query_row(
                    "SELECT previous_amount FROM observations
                     WHERE planting_id = ?1 AND kind = 'repot'
                     ORDER BY occurred_on ASC, occurred_time ASC, id ASC
                     LIMIT 1",
                    [observation.planting_id],
                    |row| row.get::<_, Option<f64>>(0),
                )
                .optional()?
                .flatten()
        } else {
            None
        };
        transaction.execute(
            "DELETE FROM media WHERE owner_type = 'observation' AND owner_id = ?1",
            [id],
        )?;
        transaction.execute("DELETE FROM observations WHERE id = ?1", [id])?;
        if observation.kind == "repot" {
            // A failed save is undone: the volume returns to the chain's last known size.
            let last = Self::rebuild_repot_chain(&transaction, observation.planting_id, initial)?;
            transaction.execute(
                "UPDATE plantings SET
                     container_volume_l = ?1,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
                 WHERE id = ?2",
                params![last, observation.planting_id],
            )?;
        }
        transaction.commit()?;

        Ok(stems)
    }

    /// Every logged stage start, oldest first. A perennial logs the same stage in more than one
    /// cycle, so the schedule matches each date to the cycle nearest to it.
    pub fn stage_starts(&self, planting_id: i64) -> Result<Vec<(i64, crate::calendar::Date)>> {
        let mut statement = self.conn.prepare(
            "SELECT stage_id, occurred_on
             FROM observations
             WHERE planting_id = ?1 AND kind = 'stage' AND stage_id IS NOT NULL
             ORDER BY occurred_on, id",
        )?;

        let rows = statement.query_map([planting_id], |row| {
            let stage_id: i64 = row.get(0)?;
            let date: String = row.get(1)?;
            let date =
                crate::calendar::Date::parse_iso(&date).ok_or_else(|| invalid_date(1, &date))?;
            Ok((stage_id, date))
        })?;

        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// A new cycle with the same setup and space. A private space is copied, because it belongs
    /// to one planting; a named space is reused.
    pub fn repeat_planting(&self, id: i64, started_on: crate::calendar::Date) -> Result<i64> {
        let source = self
            .planting(id)?
            .ok_or_else(|| ValidationError(format!("planting {id} does not exist")))?;
        let space = self.space(source.planting.space_id)?.ok_or_else(|| {
            ValidationError(format!("space {} does not exist", source.planting.space_id))
        })?;

        self.transaction(|store| {
            let space_id = if space.display_name().is_some() {
                space.id
            } else {
                store.create_space(&SpaceInput {
                    name: None,
                    kind: space.kind.clone(),
                    light_type: space.light_type.clone(),
                    light_hours: space.light_hours,
                    light_start: space.light_start,
                    notes: space.notes.clone(),
                })?
            };

            store.create_planting(&PlantingInput {
                profile_id: source.planting.profile_id,
                space_id,
                name: source.planting.name.clone(),
                quantity: source.planting.quantity,
                container: source.planting.container.clone(),
                container_volume_l: source.planting.container_volume_l,
                substrate: source.planting.substrate.clone(),
                started_on,
                start_kind: source.planting.start_kind.clone(),
                moved_on: None,
                notes: source.planting.notes.clone(),
            })
        })
    }

    /// How many plantings a profile still has; a profile with history cannot be deleted.
    pub fn planting_count_for_profile(&self, profile_id: i64) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM plantings WHERE profile_id = ?1",
            [profile_id],
            |row| row.get(0),
        )?)
    }

    pub fn create_profile(
        &self,
        name: &str,
        species: Option<&str>,
        cultivar: Option<&str>,
    ) -> Result<i64> {
        let name = name.trim();
        if name.is_empty() {
            return Err(ValidationError("profile name must not be empty".into()).into());
        }

        self.conn.execute(
            "INSERT INTO profiles (name, species, cultivar) VALUES (?1, ?2, ?3)",
            params![name, clean(species), clean(cultivar)],
        )?;

        Ok(self.conn.last_insert_rowid())
    }

    /// Copies the plant data into a new profile. History, photos and the identity of the original
    /// stay behind; values and their provenance travel with the copy.
    pub fn copy_profile(&self, id: i64) -> Result<i64> {
        self.copy_profile_named(id, None)
    }

    /// The same copy with an explicit name, or `None` for "X (copy)".
    pub fn copy_profile_named(&self, id: i64, name: Option<&str>) -> Result<i64> {
        let profile = self
            .profile(id)?
            .ok_or_else(|| ValidationError(format!("profile {id} does not exist")))?;
        let details = self
            .profile_details(id)?
            .ok_or_else(|| ValidationError(format!("profile {id} does not exist")))?;
        let stages = self.stages(id)?;
        let requirements = self.requirements(id)?;
        let timing = self.timing_rules(id)?;
        let attributes = self.attributes(id)?;
        let profile_provenance = self.provenance_for("profile", id)?;

        let copy_name = match name {
            Some(name) => {
                let name = name.trim();
                if name.is_empty() {
                    return Err(ValidationError("profile name must not be empty".into()).into());
                }
                name.to_string()
            }
            None => format!("{} (copy)", profile.name),
        };

        self.transaction(|store| {
            let new_id = store.create_profile(
                &copy_name,
                profile.species.as_deref(),
                profile.cultivar.as_deref(),
            )?;

            let mut copied_details = details.clone();
            copied_details.id = new_id;
            copied_details.name = copy_name.clone();
            store.update_profile_details(&copied_details)?;

            for provenance in &profile_provenance {
                let mut provenance = provenance.clone();
                provenance.entity_id = new_id;
                store.set_provenance(&provenance)?;
            }

            let mut stage_ids: HashMap<i64, i64> = HashMap::new();
            for stage in &stages {
                let mut copy = stage.clone();
                copy.id = 0;
                copy.profile_id = new_id;
                let stage_id = store.create_stage(&copy)?;
                stage_ids.insert(stage.id, stage_id);

                for provenance in store.provenance_for("stage", stage.id)? {
                    let mut provenance = provenance;
                    provenance.entity_id = stage_id;
                    store.set_provenance(&provenance)?;
                }
            }

            for requirement in &requirements {
                let mut copy = requirement.clone();
                copy.id = 0;
                copy.profile_id = new_id;
                copy.stage_id = copy
                    .stage_id
                    .and_then(|stage_id| stage_ids.get(&stage_id).copied());
                let requirement_id = store.set_requirement(&copy)?;

                for provenance in store.provenance_for("requirement", requirement.id)? {
                    let mut provenance = provenance;
                    provenance.entity_id = requirement_id;
                    store.set_provenance(&provenance)?;
                }
            }

            for rule in &timing {
                let mut copy = rule.clone();
                copy.id = 0;
                copy.profile_id = new_id;
                copy.stage_id = copy
                    .stage_id
                    .and_then(|stage_id| stage_ids.get(&stage_id).copied());
                let rule_id = store.create_timing_rule(&copy)?;

                for provenance in store.provenance_for("timing_rule", rule.id)? {
                    let mut provenance = provenance;
                    provenance.entity_id = rule_id;
                    store.set_provenance(&provenance)?;
                }
            }

            for attribute in &attributes {
                let mut copy = attribute.clone();
                copy.id = 0;
                copy.profile_id = new_id;
                copy.stage_id = copy
                    .stage_id
                    .and_then(|stage_id| stage_ids.get(&stage_id).copied());
                let attribute_id = store.set_attribute(&copy)?;

                for provenance in store.provenance_for("attribute", attribute.id)? {
                    let mut provenance = provenance;
                    provenance.entity_id = attribute_id;
                    store.set_provenance(&provenance)?;
                }
            }

            Ok(new_id)
        })
    }

    pub fn delete_profile(&self, id: i64) -> Result<()> {
        if self.planting_count_for_profile(id)? > 0 {
            return Err(ValidationError(
                "this plant profile still has plantings; finish or delete those first".into(),
            )
            .into());
        }

        let transaction = self.conn.unchecked_transaction()?;

        transaction.execute(
            "DELETE FROM field_provenance
             WHERE (entity_type = 'profile' AND entity_id = ?1)
                OR (entity_type = 'stage' AND entity_id IN
                    (SELECT id FROM profile_stages WHERE profile_id = ?1))
                OR (entity_type = 'requirement' AND entity_id IN
                    (SELECT id FROM profile_requirements WHERE profile_id = ?1))
                OR (entity_type = 'timing_rule' AND entity_id IN
                    (SELECT id FROM profile_timing_rules WHERE profile_id = ?1))
                OR (entity_type = 'attribute' AND entity_id IN
                    (SELECT id FROM profile_attributes WHERE profile_id = ?1))",
            [id],
        )?;
        transaction.execute("DELETE FROM changes WHERE profile_id = ?1", [id])?;
        transaction.execute(
            "DELETE FROM media WHERE owner_type = 'profile' AND owner_id = ?1",
            [id],
        )?;
        transaction.execute(
            "DELETE FROM avatar_icons WHERE owner_type = 'profile' AND owner_id = ?1",
            [id],
        )?;
        transaction.execute("DELETE FROM profiles WHERE id = ?1", [id])?;
        transaction.commit()?;

        Ok(())
    }

    pub fn update_profile(
        &self,
        id: i64,
        name: &str,
        species: Option<&str>,
        cultivar: Option<&str>,
    ) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            return Err(ValidationError("profile name must not be empty".into()).into());
        }

        self.conn.execute(
            "UPDATE profiles
             SET name = ?2,
                 species = ?3,
                 cultivar = ?4,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
             WHERE id = ?1",
            params![id, name, clean(species), clean(cultivar)],
        )?;

        Ok(())
    }

    pub fn profile_details(&self, id: i64) -> Result<Option<ProfileDetails>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, name, species, cultivar, family, genus, lifecycle, life_form, foliage,
                        determinacy, growth_habit, sun_exposure, photoperiod_type,
                        photoperiod_obligate, critical_day_length_hours, water_need,
                        container_suitable, min_container_volume_l_min, min_container_volume_l_max,
                        days_to_maturity_min, days_to_maturity_max, notes
                 FROM profiles
                 WHERE id = ?1",
                [id],
                |row| {
                    Ok(ProfileDetails {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        species: row.get(2)?,
                        cultivar: row.get(3)?,
                        family: row.get(4)?,
                        genus: row.get(5)?,
                        lifecycle: row.get(6)?,
                        life_form: row.get(7)?,
                        foliage: row.get(8)?,
                        determinacy: row.get(9)?,
                        growth_habit: row.get(10)?,
                        sun_exposure: row.get(11)?,
                        photoperiod_type: row.get(12)?,
                        photoperiod_obligate: row.get(13)?,
                        critical_day_length_hours: row.get(14)?,
                        water_need: row.get(15)?,
                        container_suitable: row.get(16)?,
                        min_container_volume_l_min: row.get(17)?,
                        min_container_volume_l_max: row.get(18)?,
                        days_to_maturity_min: row.get(19)?,
                        days_to_maturity_max: row.get(20)?,
                        notes: row.get(21)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn update_profile_details(&self, details: &ProfileDetails) -> Result<()> {
        validate_profile_details(details)?;

        let changed = self.conn.execute(
            "UPDATE profiles
             SET family = ?2,
                 genus = ?3,
                 lifecycle = ?4,
                 life_form = ?5,
                 foliage = ?6,
                 determinacy = ?7,
                 growth_habit = ?8,
                 sun_exposure = ?9,
                 photoperiod_type = ?10,
                 photoperiod_obligate = ?11,
                 critical_day_length_hours = ?12,
                 water_need = ?13,
                 container_suitable = ?14,
                 min_container_volume_l_min = ?15,
                 min_container_volume_l_max = ?16,
                 days_to_maturity_min = ?17,
                 days_to_maturity_max = ?18,
                 notes = ?19,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
             WHERE id = ?1",
            params![
                details.id,
                clean(details.family.as_deref()),
                clean(details.genus.as_deref()),
                clean(details.lifecycle.as_deref()),
                clean(details.life_form.as_deref()),
                clean(details.foliage.as_deref()),
                clean(details.determinacy.as_deref()),
                clean(details.growth_habit.as_deref()),
                clean(details.sun_exposure.as_deref()),
                clean(details.photoperiod_type.as_deref()),
                details.photoperiod_obligate,
                details.critical_day_length_hours,
                clean(details.water_need.as_deref()),
                details.container_suitable,
                details.min_container_volume_l_min,
                details.min_container_volume_l_max,
                details.days_to_maturity_min,
                details.days_to_maturity_max,
                clean(details.notes.as_deref()),
            ],
        )?;

        if changed == 0 {
            return Err(ValidationError(format!("profile {} does not exist", details.id)).into());
        }

        Ok(())
    }

    pub fn stages(&self, profile_id: i64) -> Result<Vec<Stage>> {
        let mut statement = self.conn.prepare(
            "SELECT id, profile_id, position, kind, label, duration_days_min, duration_days_max, notes, repeats
             FROM profile_stages
             WHERE profile_id = ?1
             ORDER BY position, id",
        )?;

        let rows = statement.query_map([profile_id], stage_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn stage(&self, id: i64) -> Result<Option<Stage>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, profile_id, position, kind, label, duration_days_min, duration_days_max, notes, repeats
                 FROM profile_stages
                 WHERE id = ?1",
                [id],
                stage_from_row,
            )
            .optional()?)
    }

    pub fn create_stage(&self, stage: &Stage) -> Result<i64> {
        self.validate_stage(stage)?;

        self.conn.execute(
            "INSERT INTO profile_stages
                 (profile_id, position, kind, label, duration_days_min, duration_days_max, notes,
                  repeats)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                stage.profile_id,
                stage.position,
                stage.kind,
                stage.label.trim(),
                stage.duration_days_min,
                stage.duration_days_max,
                clean(stage.notes.as_deref()),
                stage.repeats,
            ],
        )?;

        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_stage(&self, stage: &Stage) -> Result<()> {
        self.validate_stage(stage)?;

        let existing = self
            .stage(stage.id)?
            .ok_or_else(|| ValidationError(format!("stage {} does not exist", stage.id)))?;
        if existing.profile_id != stage.profile_id {
            return Err(ValidationError(format!(
                "stage {} belongs to profile {}, not {}",
                stage.id, existing.profile_id, stage.profile_id
            ))
            .into());
        }

        let changed = self.conn.execute(
            "UPDATE profile_stages
             SET position = ?2,
                 kind = ?3,
                 label = ?4,
                 duration_days_min = ?5,
                 duration_days_max = ?6,
                 notes = ?7,
                 repeats = ?8,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
             WHERE id = ?1",
            params![
                stage.id,
                stage.position,
                stage.kind,
                stage.label.trim(),
                stage.duration_days_min,
                stage.duration_days_max,
                clean(stage.notes.as_deref()),
                stage.repeats,
            ],
        )?;

        if changed == 0 {
            return Err(ValidationError(format!("stage {} does not exist", stage.id)).into());
        }

        Ok(())
    }

    /// Whether a stage still carries requirements, timing rules or attributes of its own.
    pub fn stage_has_children(&self, id: i64) -> Result<bool> {
        Ok(self.conn.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM profile_requirements WHERE stage_id = ?1
                 UNION ALL
                 SELECT 1 FROM profile_timing_rules WHERE stage_id = ?1
                 UNION ALL
                 SELECT 1 FROM profile_attributes WHERE stage_id = ?1
             )",
            [id],
            |row| row.get(0),
        )?)
    }

    pub fn delete_stage(&self, id: i64) -> Result<()> {
        if self.stage_has_children(id)? {
            return Err(ValidationError(
                "this stage still has its own values; remove those first".into(),
            )
            .into());
        }

        self.atomically(|conn| {
            conn.execute(
                "DELETE FROM field_provenance
                 WHERE (entity_type = 'stage' AND entity_id = ?1)
                    OR (entity_type = 'requirement' AND entity_id IN
                        (SELECT id FROM profile_requirements WHERE stage_id = ?1))
                    OR (entity_type = 'timing_rule' AND entity_id IN
                        (SELECT id FROM profile_timing_rules WHERE stage_id = ?1))
                    OR (entity_type = 'attribute' AND entity_id IN
                        (SELECT id FROM profile_attributes WHERE stage_id = ?1))",
                [id],
            )?;
            conn.execute("DELETE FROM profile_stages WHERE id = ?1", [id])?;
            Ok(())
        })
    }

    /// Puts the named stages first in this order and appends the rest in their current order.
    ///
    /// Positions are unique per profile, so the rows are parked on negative positions inside the
    /// same transaction before they get their final 1..n positions.
    pub fn reorder_stages(&self, profile_id: i64, ordered_ids: &[i64]) -> Result<()> {
        let stages = self.stages(profile_id)?;
        let mut all_ids: Vec<i64> = stages.iter().map(|stage| stage.id).collect();

        let mut seen = Vec::new();
        for id in ordered_ids {
            if !all_ids.contains(id) {
                return Err(ValidationError(format!(
                    "stage {id} does not belong to profile {profile_id}"
                ))
                .into());
            }
            if seen.contains(id) {
                return Err(ValidationError("a stage appears twice in the order".into()).into());
            }
            seen.push(*id);
        }

        for id in all_ids.drain(..) {
            if !seen.contains(&id) {
                seen.push(id);
            }
        }

        self.atomically(|conn| {
            for (index, id) in seen.iter().enumerate() {
                conn.execute(
                    "UPDATE profile_stages SET position = ?1 WHERE id = ?2",
                    params![-(index as i64 + 1), id],
                )?;
            }
            for (index, id) in seen.iter().enumerate() {
                conn.execute(
                    "UPDATE profile_stages
                     SET position = ?1,
                         updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
                     WHERE id = ?2",
                    params![index as i64 + 1, id],
                )?;
            }
            Ok(())
        })
    }

    /// Moves one stage one place up or down.
    pub fn move_stage(&self, id: i64, delta: i64) -> Result<()> {
        let Some(stage) = self.stage(id)? else {
            return Err(ValidationError(format!("stage {id} does not exist")).into());
        };
        let mut stages = self.stages(stage.profile_id)?;
        let Some(index) = stages.iter().position(|item| item.id == id) else {
            return Ok(());
        };
        let target = index as i64 + delta;
        if target < 0 || target >= stages.len() as i64 {
            return Ok(());
        }

        stages.swap(index, target as usize);
        let ids: Vec<i64> = stages.iter().map(|stage| stage.id).collect();
        self.reorder_stages(stage.profile_id, &ids)
    }

    pub fn requirements(&self, profile_id: i64) -> Result<Vec<Requirement>> {
        let mut statement = self.conn.prepare(
            "SELECT id, profile_id, stage_id, metric, day_part, min_value, max_value,
                    opt_min, opt_max, survival_min, survival_max
             FROM profile_requirements
             WHERE profile_id = ?1
             ORDER BY COALESCE(stage_id, 0), metric, day_part",
        )?;

        let rows = statement.query_map([profile_id], requirement_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn set_requirement(&self, requirement: &Requirement) -> Result<i64> {
        self.validate_requirement(requirement)?;

        let existing: Option<i64> = self
            .conn
            .query_row(
                "SELECT id
                 FROM profile_requirements
                 WHERE profile_id = ?1
                   AND COALESCE(stage_id, 0) = COALESCE(?2, 0)
                   AND metric = ?3
                   AND day_part = ?4",
                params![
                    requirement.profile_id,
                    requirement.stage_id,
                    requirement.metric,
                    requirement.day_part
                ],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(id) = existing {
            self.conn.execute(
                "UPDATE profile_requirements
                 SET min_value = ?2,
                     max_value = ?3,
                     opt_min = ?4,
                     opt_max = ?5,
                     survival_min = ?6,
                     survival_max = ?7,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
                 WHERE id = ?1",
                params![
                    id,
                    requirement.min,
                    requirement.max,
                    requirement.opt_min,
                    requirement.opt_max,
                    requirement.survival_min,
                    requirement.survival_max,
                ],
            )?;

            Ok(id)
        } else {
            self.conn.execute(
                "INSERT INTO profile_requirements
                     (profile_id, stage_id, metric, day_part, min_value, max_value,
                      opt_min, opt_max, survival_min, survival_max)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    requirement.profile_id,
                    requirement.stage_id,
                    requirement.metric,
                    requirement.day_part,
                    requirement.min,
                    requirement.max,
                    requirement.opt_min,
                    requirement.opt_max,
                    requirement.survival_min,
                    requirement.survival_max,
                ],
            )?;

            Ok(self.conn.last_insert_rowid())
        }
    }

    /// Resolve the requirement that applies to a profile, stage, metric and day part.
    ///
    /// A stage row wins over the profile default and `any` stands in for day and night. A row
    /// whose provenance says `not_applicable` stops the search and does not fall back to the
    /// profile default, so an explicit "not here" is different from an unknown value.
    pub fn resolved_requirement(
        &self,
        profile_id: i64,
        stage_id: Option<i64>,
        metric: &str,
        day_part: &str,
    ) -> Result<Option<RequirementResolution>> {
        validate_choice(metric, METRICS, "metric")?;
        validate_choice(day_part, DAY_PARTS, "day part")?;

        let parts: &[&str] = if day_part == "any" {
            &["any"]
        } else {
            &[day_part, "any"]
        };

        if let Some(stage_id) = stage_id {
            self.ensure_stage_in_profile(profile_id, Some(stage_id))?;

            for part in parts {
                let stage_requirement = self
                    .conn
                    .query_row(
                        "SELECT id, profile_id, stage_id, metric, day_part, min_value, max_value,
                                opt_min, opt_max, survival_min, survival_max
                         FROM profile_requirements
                         WHERE profile_id = ?1 AND stage_id = ?2 AND metric = ?3
                           AND day_part = ?4",
                        params![profile_id, stage_id, metric, part],
                        requirement_from_row,
                    )
                    .optional()?;

                if let Some(requirement) = stage_requirement {
                    return self.resolved_or_not_applicable(requirement).map(Some);
                }
            }
        }

        for part in parts {
            let profile_requirement = self
                .conn
                .query_row(
                    "SELECT id, profile_id, stage_id, metric, day_part, min_value, max_value,
                            opt_min, opt_max, survival_min, survival_max
                     FROM profile_requirements
                     WHERE profile_id = ?1 AND stage_id IS NULL AND metric = ?2
                       AND day_part = ?3",
                    params![profile_id, metric, part],
                    requirement_from_row,
                )
                .optional()?;

            if let Some(requirement) = profile_requirement {
                return self.resolved_or_not_applicable(requirement).map(Some);
            }
        }

        Ok(None)
    }

    fn resolved_or_not_applicable(
        &self,
        requirement: Requirement,
    ) -> Result<RequirementResolution> {
        let not_applicable = self
            .provenance("requirement", requirement.id, "*")?
            .is_some_and(|provenance| provenance.status == "not_applicable");

        if not_applicable {
            Ok(RequirementResolution::NotApplicable)
        } else {
            Ok(RequirementResolution::Resolved(requirement))
        }
    }

    pub fn delete_requirement(&self, id: i64) -> Result<()> {
        self.atomically(|conn| {
            conn.execute(
                "DELETE FROM field_provenance
                 WHERE entity_type = 'requirement' AND entity_id = ?1",
                [id],
            )?;
            conn.execute("DELETE FROM profile_requirements WHERE id = ?1", [id])?;
            Ok(())
        })
    }

    pub fn timing_rules(&self, profile_id: i64) -> Result<Vec<TimingRule>> {
        let mut statement = self.conn.prepare(
            "SELECT id, profile_id, stage_id, kind, anchor, offset_min_days, offset_max_days,
                    month_start, month_end, day_length_hours, gdd_base_c, gdd_target,
                    repeat_days, notes
             FROM profile_timing_rules
             WHERE profile_id = ?1
             ORDER BY COALESCE(stage_id, 0), kind, id",
        )?;

        let rows = statement.query_map([profile_id], timing_rule_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn create_timing_rule(&self, rule: &TimingRule) -> Result<i64> {
        self.validate_timing_rule(rule)?;

        self.conn.execute(
            "INSERT INTO profile_timing_rules
                 (profile_id, stage_id, kind, anchor, offset_min_days, offset_max_days,
                  month_start, month_end, day_length_hours, gdd_base_c, gdd_target,
                  repeat_days, notes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                rule.profile_id,
                rule.stage_id,
                rule.kind,
                rule.anchor,
                rule.offset_min_days,
                rule.offset_max_days,
                rule.month_start,
                rule.month_end,
                rule.day_length_hours,
                rule.gdd_base_c,
                rule.gdd_target,
                rule.repeat_days,
                clean(rule.notes.as_deref()),
            ],
        )?;

        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_timing_rule(&self, rule: &TimingRule) -> Result<()> {
        self.validate_timing_rule(rule)?;

        let changed = self.conn.execute(
            "UPDATE profile_timing_rules
             SET stage_id = ?2,
                 kind = ?3,
                 anchor = ?4,
                 offset_min_days = ?5,
                 offset_max_days = ?6,
                 month_start = ?7,
                 month_end = ?8,
                 day_length_hours = ?9,
                 gdd_base_c = ?10,
                 gdd_target = ?11,
                 repeat_days = ?12,
                 notes = ?13,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
             WHERE id = ?1",
            params![
                rule.id,
                rule.stage_id,
                rule.kind,
                rule.anchor,
                rule.offset_min_days,
                rule.offset_max_days,
                rule.month_start,
                rule.month_end,
                rule.day_length_hours,
                rule.gdd_base_c,
                rule.gdd_target,
                rule.repeat_days,
                clean(rule.notes.as_deref()),
            ],
        )?;

        if changed == 0 {
            return Err(ValidationError(format!("timing rule {} does not exist", rule.id)).into());
        }

        Ok(())
    }

    pub fn delete_timing_rule(&self, id: i64) -> Result<()> {
        self.atomically(|conn| {
            conn.execute(
                "DELETE FROM field_provenance
                 WHERE entity_type = 'timing_rule' AND entity_id = ?1",
                [id],
            )?;
            conn.execute("DELETE FROM profile_timing_rules WHERE id = ?1", [id])?;
            Ok(())
        })
    }

    pub fn attribute_definitions(&self) -> Result<Vec<AttributeDefinition>> {
        let mut statement = self.conn.prepare(
            "SELECT key, label, description, value_type, unit, applies_to, allowed_stages,
                    allowed_lifecycles, enum_values, is_custom, category
             FROM attribute_definitions
             ORDER BY is_custom, COALESCE(category, 'Other'), label COLLATE NOCASE",
        )?;

        let rows = statement.query_map([], attribute_definition_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn attribute_definition(&self, key: &str) -> Result<Option<AttributeDefinition>> {
        Ok(self
            .conn
            .query_row(
                "SELECT key, label, description, value_type, unit, applies_to, allowed_stages,
                        allowed_lifecycles, enum_values, is_custom, category
                 FROM attribute_definitions
                 WHERE key = ?1",
                [key],
                attribute_definition_from_row,
            )
            .optional()?)
    }

    pub fn add_custom_attribute_definition(&self, definition: &AttributeDefinition) -> Result<()> {
        let key = definition.key.trim();
        if key.is_empty() {
            return Err(ValidationError("attribute key must not be empty".into()).into());
        }
        if definition.label.trim().is_empty() {
            return Err(ValidationError("attribute label must not be empty".into()).into());
        }
        validate_choice(&definition.value_type, VALUE_TYPES, "attribute value type")?;
        validate_choice(&definition.applies_to, APPLIES_TO, "attribute target")?;
        validate_choice(
            &definition.category,
            ATTRIBUTE_CATEGORIES,
            "attribute category",
        )?;
        if definition.value_type == "enum" && definition.enum_values.is_none() {
            return Err(
                ValidationError("an enum attribute needs its allowed values".into()).into(),
            );
        }
        if self.attribute_definition(key)?.is_some() {
            return Err(ValidationError(format!("attribute key already exists: {key}")).into());
        }

        self.conn.execute(
            "INSERT INTO attribute_definitions
                 (key, label, description, value_type, unit, category, applies_to, enum_values,
                  is_custom)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1)",
            params![
                key,
                definition.label.trim(),
                clean(definition.description.as_deref()),
                definition.value_type,
                clean(definition.unit.as_deref()),
                definition.category,
                definition.applies_to,
                clean(definition.enum_values.as_deref()),
            ],
        )?;

        Ok(())
    }

    pub fn attributes(&self, profile_id: i64) -> Result<Vec<Attribute>> {
        let mut statement = self.conn.prepare(
            "SELECT id, profile_id, stage_id, key, value_num, value_text, value_bool
             FROM profile_attributes
             WHERE profile_id = ?1
             ORDER BY COALESCE(stage_id, 0), key",
        )?;

        let rows = statement.query_map([profile_id], attribute_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn set_attribute(&self, attribute: &Attribute) -> Result<i64> {
        self.validate_attribute(attribute)?;

        let existing: Option<i64> = self
            .conn
            .query_row(
                "SELECT id
                 FROM profile_attributes
                 WHERE profile_id = ?1
                   AND COALESCE(stage_id, 0) = COALESCE(?2, 0)
                   AND key = ?3",
                params![attribute.profile_id, attribute.stage_id, attribute.key],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(id) = existing {
            self.conn.execute(
                "UPDATE profile_attributes
                 SET value_num = ?2,
                     value_text = ?3,
                     value_bool = ?4,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
                 WHERE id = ?1",
                params![
                    id,
                    attribute.value_num,
                    clean(attribute.value_text.as_deref()),
                    attribute.value_bool,
                ],
            )?;

            Ok(id)
        } else {
            self.conn.execute(
                "INSERT INTO profile_attributes
                     (profile_id, stage_id, key, value_num, value_text, value_bool)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    attribute.profile_id,
                    attribute.stage_id,
                    attribute.key,
                    attribute.value_num,
                    clean(attribute.value_text.as_deref()),
                    attribute.value_bool,
                ],
            )?;

            Ok(self.conn.last_insert_rowid())
        }
    }

    pub fn delete_attribute(&self, id: i64) -> Result<()> {
        self.atomically(|conn| {
            conn.execute(
                "DELETE FROM field_provenance
                 WHERE entity_type = 'attribute' AND entity_id = ?1",
                [id],
            )?;
            conn.execute("DELETE FROM profile_attributes WHERE id = ?1", [id])?;
            Ok(())
        })
    }

    pub fn set_provenance(&self, provenance: &Provenance) -> Result<()> {
        validate_provenance(provenance)?;

        self.conn.execute(
            "INSERT INTO field_provenance
                 (entity_type, entity_id, field, status, source_type, source_ref,
                  retrieved_at, confidence, notes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(entity_type, entity_id, field) DO UPDATE SET
                 status = excluded.status,
                 source_type = excluded.source_type,
                 source_ref = excluded.source_ref,
                 retrieved_at = excluded.retrieved_at,
                 confidence = excluded.confidence,
                 notes = excluded.notes,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')",
            params![
                provenance.entity_type,
                provenance.entity_id,
                provenance.field,
                provenance.status,
                clean(provenance.source_type.as_deref()),
                clean(provenance.source_ref.as_deref()),
                clean(provenance.retrieved_at.as_deref()),
                clean(provenance.confidence.as_deref()),
                clean(provenance.notes.as_deref()),
            ],
        )?;

        Ok(())
    }

    pub fn provenance(
        &self,
        entity_type: &str,
        entity_id: i64,
        field: &str,
    ) -> Result<Option<Provenance>> {
        Ok(self
            .conn
            .query_row(
                "SELECT entity_type, entity_id, field, status, source_type, source_ref,
                        retrieved_at, confidence, notes
                 FROM field_provenance
                 WHERE entity_type = ?1 AND entity_id = ?2 AND field = ?3",
                params![entity_type, entity_id, field],
                provenance_from_row,
            )
            .optional()?)
    }

    /// Every provenance row for one entity, ordered by field.
    pub fn provenance_for(&self, entity_type: &str, entity_id: i64) -> Result<Vec<Provenance>> {
        let mut statement = self.conn.prepare(
            "SELECT entity_type, entity_id, field, status, source_type, source_ref,
                    retrieved_at, confidence, notes
             FROM field_provenance
             WHERE entity_type = ?1 AND entity_id = ?2
             ORDER BY field",
        )?;

        let rows = statement.query_map(params![entity_type, entity_id], provenance_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn media(&self, owner_type: &str, owner_id: i64, role: &str) -> Result<Option<Media>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, owner_type, owner_id, role, file_stem, width, height, bytes
                 FROM media
                 WHERE owner_type = ?1 AND owner_id = ?2 AND role = ?3
                 ORDER BY id
                 LIMIT 1",
                params![owner_type, owner_id, role],
                media_from_row,
            )
            .optional()?)
    }

    /// The icon a profile, planting or space shows instead of its photo.
    pub fn avatar_icon(&self, owner_type: &str, owner_id: i64) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT icon FROM avatar_icons
                 WHERE owner_type = ?1 AND owner_id = ?2",
                params![owner_type, owner_id],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// Sets or clears the chosen avatar illustration. Clearing it returns the owner to its photo
    /// or fallback picture.
    pub fn set_avatar_icon(
        &self,
        owner_type: &str,
        owner_id: i64,
        icon: Option<&str>,
    ) -> Result<()> {
        validate_avatar_owner(owner_type, owner_id)?;

        match icon {
            Some(icon) => {
                validate_avatar_icon(icon)?;
                self.conn.execute(
                    "INSERT INTO avatar_icons (owner_type, owner_id, icon)
                     VALUES (?1, ?2, ?3)
                     ON CONFLICT(owner_type, owner_id) DO UPDATE SET
                         icon = excluded.icon,
                         updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')",
                    params![owner_type, owner_id, icon],
                )?;
            }
            None => {
                self.conn.execute(
                    "DELETE FROM avatar_icons WHERE owner_type = ?1 AND owner_id = ?2",
                    params![owner_type, owner_id],
                )?;
            }
        }

        Ok(())
    }

    /// Every image for one owner, oldest first.
    pub fn media_for(&self, owner_type: &str, owner_id: i64) -> Result<Vec<Media>> {
        let mut statement = self.conn.prepare(
            "SELECT id, owner_type, owner_id, role, file_stem, width, height, bytes
             FROM media
             WHERE owner_type = ?1 AND owner_id = ?2
             ORDER BY id",
        )?;

        let rows = statement.query_map(params![owner_type, owner_id], media_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Keeps one row per owner and role: an existing image is updated in place.
    ///
    /// This fits a cover. A gallery needs an explicit insert per photo instead of reaching for
    /// this method, which always updates the first row for the owner and role.
    pub fn set_media(&self, media: &Media) -> Result<i64> {
        validate_media(media)?;

        let existing: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM media
                 WHERE owner_type = ?1 AND owner_id = ?2 AND role = ?3
                 ORDER BY id
                 LIMIT 1",
                params![media.owner_type, media.owner_id, media.role],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(id) = existing {
            self.conn.execute(
                "UPDATE media
                 SET file_stem = ?2, width = ?3, height = ?4, bytes = ?5,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
                 WHERE id = ?1",
                params![id, media.file_stem, media.width, media.height, media.bytes],
            )?;

            Ok(id)
        } else {
            self.conn.execute(
                "INSERT INTO media (owner_type, owner_id, role, file_stem, width, height, bytes)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    media.owner_type,
                    media.owner_id,
                    media.role,
                    media.file_stem,
                    media.width,
                    media.height,
                    media.bytes
                ],
            )?;

            Ok(self.conn.last_insert_rowid())
        }
    }

    /// Removes the rows for an owner and role. The caller removes the files.
    pub fn delete_media(&self, owner_type: &str, owner_id: i64, role: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM media WHERE owner_type = ?1 AND owner_id = ?2 AND role = ?3",
            params![owner_type, owner_id, role],
        )?;

        Ok(())
    }

    pub fn record_change(&self, change: &Change) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO changes
                 (profile_id, entity_type, entity_id, field, old_value, new_value,
                  source_type, source_ref, actor)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                change.profile_id,
                change.entity_type,
                change.entity_id,
                change.field,
                change.old_value,
                change.new_value,
                change.source_type,
                change.source_ref,
                change.actor,
            ],
        )?;

        Ok(self.conn.last_insert_rowid())
    }

    /// The newest changes for a profile, newest first.
    pub fn changes(&self, profile_id: i64, limit: i64) -> Result<Vec<Change>> {
        let mut statement = self.conn.prepare(
            "SELECT id, profile_id, entity_type, entity_id, field, old_value, new_value,
                    source_type, source_ref, actor, created_at
             FROM changes
             WHERE profile_id = ?1
             ORDER BY id DESC
             LIMIT ?2",
        )?;

        let rows = statement.query_map(params![profile_id, limit], |row| {
            Ok(Change {
                id: row.get(0)?,
                profile_id: row.get(1)?,
                entity_type: row.get(2)?,
                entity_id: row.get(3)?,
                field: row.get(4)?,
                old_value: row.get(5)?,
                new_value: row.get(6)?,
                source_type: row.get(7)?,
                source_ref: row.get(8)?,
                actor: row.get(9)?,
                created_at: row.get(10)?,
            })
        })?;

        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// How many values in this profile have each provenance status. Used by the status card.
    pub fn provenance_status_counts(&self, profile_id: i64) -> Result<Vec<(String, i64)>> {
        let mut statement = self.conn.prepare(
            "SELECT status, COUNT(*)
             FROM field_provenance
             WHERE (entity_type = 'profile' AND entity_id = ?1)
                OR (entity_type = 'stage' AND entity_id IN
                    (SELECT id FROM profile_stages WHERE profile_id = ?1))
                OR (entity_type = 'requirement' AND entity_id IN
                    (SELECT id FROM profile_requirements WHERE profile_id = ?1))
                OR (entity_type = 'timing_rule' AND entity_id IN
                    (SELECT id FROM profile_timing_rules WHERE profile_id = ?1))
                OR (entity_type = 'attribute' AND entity_id IN
                    (SELECT id FROM profile_attributes WHERE profile_id = ?1))
             GROUP BY status",
        )?;

        let rows = statement.query_map([profile_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn delete_provenance(&self, entity_type: &str, entity_id: i64, field: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM field_provenance
             WHERE entity_type = ?1 AND entity_id = ?2 AND field = ?3",
            params![entity_type, entity_id, field],
        )?;

        Ok(())
    }

    fn ensure_profile(&self, profile_id: i64) -> Result<()> {
        if self.profile(profile_id)?.is_none() {
            return Err(ValidationError(format!("profile {profile_id} does not exist")).into());
        }

        Ok(())
    }

    fn ensure_stage_in_profile(&self, profile_id: i64, stage_id: Option<i64>) -> Result<()> {
        let Some(stage_id) = stage_id else {
            return Ok(());
        };

        let stage = self
            .stage(stage_id)?
            .ok_or_else(|| ValidationError(format!("stage {stage_id} does not exist")))?;
        if stage.profile_id != profile_id {
            return Err(ValidationError(format!(
                "stage {stage_id} belongs to profile {}, not {profile_id}",
                stage.profile_id
            ))
            .into());
        }

        Ok(())
    }

    fn validate_stage(&self, stage: &Stage) -> Result<()> {
        self.ensure_profile(stage.profile_id)?;
        validate_choice(&stage.kind, STAGE_KINDS, "stage kind")?;

        if stage.label.trim().is_empty() {
            return Err(ValidationError("stage label must not be empty".into()).into());
        }
        if stage.position < 0 {
            return Err(ValidationError("stage position must not be negative".into()).into());
        }

        validate_optional_range(
            stage.duration_days_min,
            stage.duration_days_max,
            "stage duration",
        )?;
        validate_non_negative(stage.duration_days_min, "stage duration")?;
        validate_non_negative(stage.duration_days_max, "stage duration")?;

        Ok(())
    }

    fn validate_requirement(&self, requirement: &Requirement) -> Result<()> {
        self.ensure_profile(requirement.profile_id)?;
        self.ensure_stage_in_profile(requirement.profile_id, requirement.stage_id)?;
        validate_choice(&requirement.metric, METRICS, "metric")?;
        validate_choice(&requirement.day_part, DAY_PARTS, "day part")?;

        let has_bound = [
            requirement.min,
            requirement.max,
            requirement.opt_min,
            requirement.opt_max,
            requirement.survival_min,
            requirement.survival_max,
        ]
        .into_iter()
        .any(|bound| bound.is_some());

        if !has_bound {
            return Err(ValidationError("a requirement needs at least one bound".into()).into());
        }

        validate_requirement_range(requirement)?;
        validate_metric_bounds(requirement)?;

        Ok(())
    }

    fn validate_timing_rule(&self, rule: &TimingRule) -> Result<()> {
        self.ensure_profile(rule.profile_id)?;
        self.ensure_stage_in_profile(rule.profile_id, rule.stage_id)?;
        validate_choice(&rule.kind, TIMING_KINDS, "timing kind")?;
        validate_choice(&rule.anchor, TIMING_ANCHORS, "timing anchor")?;

        if let (Some(min), Some(max)) = (rule.offset_min_days, rule.offset_max_days)
            && min > max
        {
            return Err(
                ValidationError("timing offset minimum must not exceed maximum".into()).into(),
            );
        }

        for offset in [rule.offset_min_days, rule.offset_max_days]
            .into_iter()
            .flatten()
        {
            if !(-3650..=3650).contains(&offset) {
                return Err(ValidationError(
                    "a timing offset must stay within ten years of its anchor".into(),
                )
                .into());
            }
        }

        for month in [rule.month_start, rule.month_end].into_iter().flatten() {
            if !(1..=12).contains(&month) {
                return Err(ValidationError("months must be between 1 and 12".into()).into());
            }
        }

        if let Some(hours) = rule.day_length_hours
            && (!hours.is_finite() || !(0.0..=24.0).contains(&hours))
        {
            return Err(ValidationError("day length must be between 0 and 24 hours".into()).into());
        }

        if let Some(base) = rule.gdd_base_c
            && !base.is_finite()
        {
            return Err(ValidationError("GDD base temperature must be finite".into()).into());
        }

        validate_non_negative(rule.gdd_target, "GDD target")?;

        if let Some(days) = rule.repeat_days
            && days <= 0
        {
            return Err(ValidationError("repeat interval must be positive".into()).into());
        }

        match rule.anchor.as_str() {
            "calendar" if rule.month_start.is_none() || rule.month_end.is_none() => {
                return Err(
                    ValidationError("a calendar rule needs a start and end month".into()).into(),
                );
            }
            "day_length" if rule.day_length_hours.is_none() => {
                return Err(ValidationError("a day-length rule needs a threshold".into()).into());
            }
            "gdd" if rule.gdd_target.is_none() => {
                return Err(ValidationError("a GDD rule needs a target".into()).into());
            }
            _ => {}
        }

        Ok(())
    }

    fn validate_attribute(&self, attribute: &Attribute) -> Result<()> {
        self.ensure_profile(attribute.profile_id)?;
        self.ensure_stage_in_profile(attribute.profile_id, attribute.stage_id)?;

        let definition = self
            .attribute_definition(&attribute.key)?
            .ok_or_else(|| ValidationError(format!("unknown attribute key: {}", attribute.key)))?;

        match definition.applies_to.as_str() {
            "profile" if attribute.stage_id.is_some() => {
                return Err(ValidationError(format!(
                    "attribute {} applies to the profile, not to a stage",
                    definition.key
                ))
                .into());
            }
            "stage" if attribute.stage_id.is_none() => {
                return Err(ValidationError(format!(
                    "attribute {} applies to a stage; choose one",
                    definition.key
                ))
                .into());
            }
            "profile" | "stage" => {}
            _ => {
                return Err(ValidationError(format!(
                    "attribute {} does not apply to a profile",
                    definition.key
                ))
                .into());
            }
        }

        let value_count = [
            attribute.value_num.is_some(),
            attribute.value_text.is_some(),
            attribute.value_bool.is_some(),
        ]
        .into_iter()
        .filter(|present| *present)
        .count();

        if value_count != 1 {
            return Err(ValidationError("an attribute needs exactly one value".into()).into());
        }

        match definition.value_type.as_str() {
            "number" => {
                let Some(value) = attribute.value_num else {
                    return Err(ValidationError(format!(
                        "attribute {} needs a number",
                        definition.key
                    ))
                    .into());
                };
                if !value.is_finite() {
                    return Err(ValidationError("attribute number must be finite".into()).into());
                }
            }
            "text" | "json" => {
                if attribute
                    .value_text
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .is_none()
                {
                    return Err(ValidationError(format!(
                        "attribute {} needs text",
                        definition.key
                    ))
                    .into());
                }
            }
            "boolean" => {
                if attribute.value_bool.is_none() {
                    return Err(ValidationError(format!(
                        "attribute {} needs a boolean",
                        definition.key
                    ))
                    .into());
                }
            }
            "enum" => {
                let Some(value) = attribute.value_text.as_deref() else {
                    return Err(ValidationError(format!(
                        "attribute {} needs an enum value",
                        definition.key
                    ))
                    .into());
                };
                if let Some(allowed) = definition.enum_values.as_deref()
                    && !allowed.split(',').any(|item| item.trim() == value)
                {
                    return Err(ValidationError(format!(
                        "invalid value {value:?} for {}; expected one of {allowed}",
                        definition.key
                    ))
                    .into());
                }
            }
            other => {
                return Err(
                    ValidationError(format!("unsupported attribute value type: {other}")).into(),
                );
            }
        }

        Ok(())
    }
}

fn stage_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Stage> {
    Ok(Stage {
        id: row.get(0)?,
        profile_id: row.get(1)?,
        position: row.get(2)?,
        kind: row.get(3)?,
        label: row.get(4)?,
        duration_days_min: row.get(5)?,
        duration_days_max: row.get(6)?,
        notes: row.get(7)?,
        repeats: row.get(8)?,
    })
}

fn requirement_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Requirement> {
    Ok(Requirement {
        id: row.get(0)?,
        profile_id: row.get(1)?,
        stage_id: row.get(2)?,
        metric: row.get(3)?,
        day_part: row.get(4)?,
        min: row.get(5)?,
        max: row.get(6)?,
        opt_min: row.get(7)?,
        opt_max: row.get(8)?,
        survival_min: row.get(9)?,
        survival_max: row.get(10)?,
    })
}

fn timing_rule_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TimingRule> {
    Ok(TimingRule {
        id: row.get(0)?,
        profile_id: row.get(1)?,
        stage_id: row.get(2)?,
        kind: row.get(3)?,
        anchor: row.get(4)?,
        offset_min_days: row.get(5)?,
        offset_max_days: row.get(6)?,
        month_start: row.get(7)?,
        month_end: row.get(8)?,
        day_length_hours: row.get(9)?,
        gdd_base_c: row.get(10)?,
        gdd_target: row.get(11)?,
        repeat_days: row.get(12)?,
        notes: row.get(13)?,
    })
}

fn media_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Media> {
    Ok(Media {
        id: row.get(0)?,
        owner_type: row.get(1)?,
        owner_id: row.get(2)?,
        role: row.get(3)?,
        file_stem: row.get(4)?,
        width: row.get(5)?,
        height: row.get(6)?,
        bytes: row.get(7)?,
    })
}

fn attribute_definition_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AttributeDefinition> {
    Ok(AttributeDefinition {
        key: row.get(0)?,
        label: row.get(1)?,
        description: row.get(2)?,
        value_type: row.get(3)?,
        unit: row.get(4)?,
        applies_to: row.get(5)?,
        allowed_stages: row.get(6)?,
        allowed_lifecycles: row.get(7)?,
        enum_values: row.get(8)?,
        is_custom: row.get(9)?,
        category: row
            .get::<_, Option<String>>(10)?
            .unwrap_or_else(|| "Other".to_string()),
    })
}

fn attribute_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Attribute> {
    Ok(Attribute {
        id: row.get(0)?,
        profile_id: row.get(1)?,
        stage_id: row.get(2)?,
        key: row.get(3)?,
        value_num: row.get(4)?,
        value_text: row.get(5)?,
        value_bool: row.get(6)?,
    })
}

fn provenance_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Provenance> {
    Ok(Provenance {
        entity_type: row.get(0)?,
        entity_id: row.get(1)?,
        field: row.get(2)?,
        status: row.get(3)?,
        source_type: row.get(4)?,
        source_ref: row.get(5)?,
        retrieved_at: row.get(6)?,
        confidence: row.get(7)?,
        notes: row.get(8)?,
    })
}

fn validate_profile_details(details: &ProfileDetails) -> Result<()> {
    validate_optional_choice(details.lifecycle.as_deref(), LIFECYCLES, "lifecycle")?;
    validate_optional_choice(details.life_form.as_deref(), LIFE_FORMS, "life form")?;
    validate_optional_choice(details.foliage.as_deref(), FOLIAGE, "foliage")?;
    validate_optional_choice(details.determinacy.as_deref(), DETERMINACY, "determinacy")?;
    validate_optional_choice(
        details.growth_habit.as_deref(),
        GROWTH_HABITS,
        "growth habit",
    )?;
    validate_optional_choice(
        details.sun_exposure.as_deref(),
        SUN_EXPOSURES,
        "sun exposure",
    )?;
    validate_optional_choice(
        details.photoperiod_type.as_deref(),
        PHOTOPERIOD_TYPES,
        "photoperiod type",
    )?;
    validate_optional_choice(details.water_need.as_deref(), WATER_NEEDS, "water need")?;

    validate_optional_range(
        details.min_container_volume_l_min,
        details.min_container_volume_l_max,
        "container volume",
    )?;
    validate_non_negative(details.min_container_volume_l_min, "container volume")?;
    validate_non_negative(details.min_container_volume_l_max, "container volume")?;

    validate_optional_range(
        details.days_to_maturity_min,
        details.days_to_maturity_max,
        "days to maturity",
    )?;
    validate_non_negative(details.days_to_maturity_min, "days to maturity")?;
    validate_non_negative(details.days_to_maturity_max, "days to maturity")?;

    if let Some(hours) = details.critical_day_length_hours
        && (!hours.is_finite() || !(0.0..=24.0).contains(&hours))
    {
        return Err(
            ValidationError("critical day length must be between 0 and 24 hours".into()).into(),
        );
    }

    Ok(())
}

fn validate_metric_bounds(requirement: &Requirement) -> Result<()> {
    let values = [
        requirement.survival_min,
        requirement.min,
        requirement.opt_min,
        requirement.opt_max,
        requirement.max,
        requirement.survival_max,
    ];

    let check = |low: f64, high: f64, label: &str| -> Result<()> {
        for value in values.into_iter().flatten() {
            if !(low..=high).contains(&value) {
                return Err(
                    ValidationError(format!("{label} must be between {low} and {high}")).into(),
                );
            }
        }
        Ok(())
    };

    match requirement.metric.as_str() {
        "air_temp" | "soil_temp" => check(
            crate::plant_data::TEMPERATURE_MIN_C,
            crate::plant_data::TEMPERATURE_MAX_C,
            "temperature (°C)",
        )?,
        "rh" => check(0.0, 100.0, "relative humidity")?,
        "soil_ph" => check(0.0, 14.0, "pH")?,
        "photoperiod" | "direct_sun_hours" | "leaf_wetness_hours" => check(0.0, 24.0, "hours")?,
        "vpd" | "light_dli" | "light_ppfd" | "soil_ec" | "water_mm_week" | "water_l_week"
        | "co2" => {
            for value in values.into_iter().flatten() {
                if value < 0.0 {
                    return Err(ValidationError(format!(
                        "{} must not be negative",
                        requirement.metric
                    ))
                    .into());
                }
            }
        }
        _ => {}
    }

    Ok(())
}

fn validate_media(media: &Media) -> Result<()> {
    if media.owner_type.trim().is_empty() {
        return Err(ValidationError("an image needs an owner type".into()).into());
    }
    if media.owner_id <= 0 {
        return Err(ValidationError("an image needs an owner".into()).into());
    }
    if media.role.trim().is_empty() || media.file_stem.trim().is_empty() {
        return Err(ValidationError("an image needs a role and a file".into()).into());
    }
    if !media
        .file_stem
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '-' || character == '_')
    {
        return Err(ValidationError(
            "an image file name may only contain letters, digits, dashes and underscores".into(),
        )
        .into());
    }
    if media.width <= 0 || media.height <= 0 {
        return Err(ValidationError("an image needs a size".into()).into());
    }
    if media.bytes < 0 {
        return Err(ValidationError("an image size cannot be negative".into()).into());
    }

    Ok(())
}

const AVATAR_OWNERS: &[&str] = &["profile", "planting", "space"];

fn validate_avatar_owner(owner_type: &str, owner_id: i64) -> Result<()> {
    if !AVATAR_OWNERS.contains(&owner_type) {
        return Err(ValidationError(format!("unknown avatar owner “{owner_type}”")).into());
    }
    if owner_id <= 0 {
        return Err(ValidationError("an avatar needs an owner".into()).into());
    }
    Ok(())
}

fn validate_avatar_icon(icon: &str) -> Result<()> {
    if icon.trim().is_empty() {
        return Err(ValidationError("an avatar icon needs a name".into()).into());
    }
    if !icon.chars().all(|character| {
        character.is_ascii_alphanumeric()
            || character == '-'
            || character == '_'
            || character == ':'
    }) {
        return Err(ValidationError(
            "an avatar icon may only contain letters, digits, dashes, underscores and colons"
                .into(),
        )
        .into());
    }
    Ok(())
}

fn validate_provenance(provenance: &Provenance) -> Result<()> {
    validate_choice(&provenance.status, PROVENANCE_STATUSES, "provenance status")?;
    validate_optional_choice(
        provenance.source_type.as_deref(),
        SOURCE_TYPES,
        "source type",
    )?;
    validate_optional_choice(
        provenance.confidence.as_deref(),
        CONFIDENCE_LEVELS,
        "confidence",
    )?;

    if provenance.status == "estimated" && provenance.source_type.is_none() {
        return Err(ValidationError("an estimated value needs a source".into()).into());
    }

    Ok(())
}

fn clean(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

const PLANTING_SELECT: &str = "SELECT
        p.id, p.profile_id, p.space_id, p.name, p.quantity, p.container,
        p.container_volume_l, p.substrate, p.started_on, p.start_kind,
        p.ended_on, p.outcome, p.notes,
        pr.name, pr.species, pr.cultivar, s.name
     FROM plantings p
     JOIN profiles pr ON pr.id = p.profile_id
     JOIN spaces s ON s.id = p.space_id";

fn space_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Space> {
    Ok(Space {
        id: row.get(0)?,
        name: row.get(1)?,
        kind: row.get(2)?,
        light_type: row.get(3)?,
        light_hours: row.get(4)?,
        light_start: row.get(5)?,
        notes: row.get(6)?,
    })
}

fn climate_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ClimateReading> {
    Ok(ClimateReading {
        id: row.get(0)?,
        space_id: row.get(1)?,
        measured_on: iso_column(row, 2)?,
        measured_time: row.get(3)?,
        temperature_c: row.get(4)?,
        humidity_pct: row.get(5)?,
        source: row.get(6)?,
        source_ref: row.get(7)?,
    })
}

fn weather_current_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WeatherCurrent> {
    let is_day: Option<i64> = row.get(12)?;

    Ok(WeatherCurrent {
        latitude: row.get(0)?,
        longitude: row.get(1)?,
        observed_on: iso_column(row, 2)?,
        observed_time: row.get(3)?,
        temperature_c: row.get(4)?,
        humidity_pct: row.get(5)?,
        apparent_temperature_c: row.get(6)?,
        weather_code: row.get(7)?,
        precipitation_mm: row.get(8)?,
        cloud_cover_pct: row.get(9)?,
        wind_speed_kmh: row.get(10)?,
        wind_gust_kmh: row.get(11)?,
        is_day: is_day.map(|value| value == 1),
    })
}

fn placement_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PlantingPlacement> {
    Ok(PlantingPlacement {
        id: row.get(0)?,
        planting_id: row.get(1)?,
        space_id: row.get(2)?,
        started_on: iso_column(row, 3)?,
        ended_on: optional_iso_column(row, 4)?,
        space_kind: row.get(5)?,
        light_type: row.get(6)?,
        light_hours: row.get(7)?,
        light_start: row.get(8)?,
    })
}

fn weather_day_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WeatherDay> {
    Ok(WeatherDay {
        latitude: row.get(0)?,
        longitude: row.get(1)?,
        date: iso_column(row, 2)?,
        temp_min_c: row.get(3)?,
        temp_max_c: row.get(4)?,
        temp_mean_c: row.get(5)?,
        humidity_mean_pct: row.get(6)?,
        humidity_min_pct: row.get(7)?,
        humidity_max_pct: row.get(8)?,
        precipitation_mm: row.get(9)?,
        precipitation_hours: row.get(10)?,
        weather_code: row.get(11)?,
        sunshine_hours: row.get(12)?,
        daylight_hours: row.get(13)?,
        radiation_sum_mj_m2: row.get(14)?,
        wind_max_kmh: row.get(15)?,
        wind_gust_max_kmh: row.get(16)?,
        et0_mm: row.get(17)?,
        sunrise: row.get(18)?,
        sunset: row.get(19)?,
        night_start: row.get(20)?,
        night_end: row.get(21)?,
        night_temp_avg_c: row.get(22)?,
        night_temp_min_c: row.get(23)?,
        night_humidity_avg_pct: row.get(24)?,
        night_hours: row.get(25)?,
        night_complete: row.get::<_, Option<String>>(20)?.is_some(),
    })
}

fn weather_hour_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WeatherHour> {
    let time: String = row.get(3)?;
    let minutes = time
        .split_once(':')
        .and_then(|(hours, minutes)| {
            Some(hours.parse::<i64>().ok()? * 60 + minutes.parse::<i64>().ok()?)
        })
        .unwrap_or(0);

    Ok(WeatherHour {
        latitude: row.get(0)?,
        longitude: row.get(1)?,
        date: iso_column(row, 2)?,
        time,
        minutes,
        temperature_c: row.get(4)?,
        humidity_pct: row.get(5)?,
        precipitation_mm: row.get(6)?,
        weather_code: row.get(7)?,
        cloud_cover_pct: row.get(8)?,
        sunshine_hours: row.get(9)?,
        is_day: row.get::<_, Option<i64>>(10)?.map(|value| value == 1),
    })
}

fn observation_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Observation> {
    Ok(Observation {
        id: row.get(0)?,
        planting_id: row.get(1)?,
        kind: row.get(2)?,
        occurred_on: iso_column(row, 3)?,
        occurred_time: row.get(4)?,
        note: row.get(5)?,
        amount: row.get(6)?,
        unit: row.get(7)?,
        stage_id: row.get(8)?,
        previous_amount: row.get(9)?,
        previous_unit: row.get(10)?,
    })
}

fn planting_summary_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PlantingSummary> {
    Ok(PlantingSummary {
        planting: Planting {
            id: row.get(0)?,
            profile_id: row.get(1)?,
            space_id: row.get(2)?,
            name: row.get(3)?,
            quantity: row.get(4)?,
            container: row.get(5)?,
            container_volume_l: row.get(6)?,
            substrate: row.get(7)?,
            started_on: iso_column(row, 8)?,
            start_kind: row.get(9)?,
            ended_on: optional_iso_column(row, 10)?,
            outcome: row.get(11)?,
            notes: row.get(12)?,
        },
        profile_name: row.get(13)?,
        profile_species: row.get(14)?,
        profile_cultivar: row.get(15)?,
        space_name: row.get(16)?,
    })
}

fn iso_column(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<crate::calendar::Date> {
    let text: String = row.get(index)?;
    crate::calendar::Date::parse_iso(&text).ok_or_else(|| invalid_date(index, &text))
}

fn optional_iso_column(
    row: &rusqlite::Row<'_>,
    index: usize,
) -> rusqlite::Result<Option<crate::calendar::Date>> {
    let text: Option<String> = row.get(index)?;
    match text.as_deref().map(str::trim) {
        Some(text) if !text.is_empty() => crate::calendar::Date::parse_iso(text)
            .map(Some)
            .ok_or_else(|| invalid_date(index, text)),
        _ => Ok(None),
    }
}

fn invalid_date(index: usize, text: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        index,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid stored date {text:?}"),
        )),
    )
}

pub fn data_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_DATA_HOME") {
        let dir = PathBuf::from(dir);
        if dir.is_absolute() {
            return Some(dir.join("omagrow"));
        }
    }

    let home = std::env::var_os("HOME")?;
    if home.is_empty() {
        return None;
    }

    Some(PathBuf::from(home).join(".local/share/omagrow"))
}

/// Never fall back to a relative path: a reader in a sandbox would write into the current
/// directory instead of failing.
fn no_data_dir() -> Box<dyn std::error::Error> {
    ValidationError("no data directory: set XDG_DATA_HOME or HOME".into()).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_and_reads_profiles() {
        let store = Store::open_in_memory().unwrap();
        let id = store
            .create_profile(
                "Bhut Jolokia",
                Some("Capsicum chinense"),
                Some("Bhut Jolokia"),
            )
            .unwrap();

        let profile = store.profile(id).unwrap().unwrap();
        assert_eq!(profile.name, "Bhut Jolokia");
        assert_eq!(profile.species.as_deref(), Some("Capsicum chinense"));
        assert_eq!(profile.cultivar.as_deref(), Some("Bhut Jolokia"));
    }

    #[test]
    fn blank_optional_fields_are_stored_as_null() {
        let store = Store::open_in_memory().unwrap();
        let id = store.create_profile("Mint", Some("  "), None).unwrap();

        let profile = store.profile(id).unwrap().unwrap();
        assert_eq!(profile.species, None);
        assert_eq!(profile.cultivar, None);
    }

    #[test]
    fn trims_the_name() {
        let store = Store::open_in_memory().unwrap();
        let id = store.create_profile("  Tomato ", None, None).unwrap();

        assert_eq!(store.profile(id).unwrap().unwrap().name, "Tomato");
    }

    #[test]
    fn lists_profiles_by_name_case_insensitively() {
        let store = Store::open_in_memory().unwrap();
        store.create_profile("basil", None, None).unwrap();
        store.create_profile("Apple", None, None).unwrap();

        let names: Vec<String> = store
            .profiles()
            .unwrap()
            .into_iter()
            .map(|profile| profile.name)
            .collect();

        assert_eq!(names, ["Apple", "basil"]);
    }

    #[test]
    fn renames_latin_name_to_species() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE profiles (
                 id         INTEGER PRIMARY KEY AUTOINCREMENT,
                 name       TEXT NOT NULL,
                 latin_name TEXT,
                 cultivar   TEXT,
                 created_at TEXT NOT NULL DEFAULT '',
                 updated_at TEXT NOT NULL DEFAULT ''
             );
             CREATE TABLE locations (
                 id         INTEGER PRIMARY KEY CHECK (id = 1),
                 latitude   REAL NOT NULL,
                 longitude  REAL NOT NULL,
                 updated_at TEXT NOT NULL DEFAULT ''
             );
             PRAGMA user_version = 2;",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO profiles (name, latin_name) VALUES ('Basil', 'Ocimum basilicum')",
            [],
        )
        .unwrap();

        let store = Store::prepare(conn).unwrap();
        let profile = store.profile(1).unwrap().unwrap();
        assert_eq!(profile.species.as_deref(), Some("Ocimum basilicum"));
    }

    #[test]
    fn counts_provenance_statuses_for_the_trust_line() {
        let store = Store::open_in_memory().unwrap();
        let id = store.create_profile("Apple", None, None).unwrap();

        let mut estimated = confirmed_provenance("profile", id);
        estimated.field = "lifecycle".to_string();
        estimated.status = "estimated".to_string();
        estimated.source_type = Some("website".to_string());
        estimated.source_ref = Some("https://example.org".to_string());
        store.set_provenance(&estimated).unwrap();

        let mut confirmed = confirmed_provenance("profile", id);
        confirmed.field = "family".to_string();
        store.set_provenance(&confirmed).unwrap();

        let counts = store.provenance_status_counts(id).unwrap();
        assert!(counts.contains(&("estimated".to_string(), 1)));
        assert!(counts.contains(&("confirmed".to_string(), 1)));
    }

    #[test]
    fn deletes_a_profile() {
        let store = Store::open_in_memory().unwrap();
        let keep = store.create_profile("Basil", None, None).unwrap();
        let drop = store.create_profile("Mint", None, None).unwrap();

        store.delete_profile(drop).unwrap();

        assert_eq!(store.profile(drop).unwrap(), None);
        assert_eq!(store.profile(keep).unwrap().unwrap().name, "Basil");
        assert_eq!(store.profiles().unwrap().len(), 1);
    }

    #[test]
    fn stores_one_location() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(store.location().unwrap(), None);

        let lisbon = Location {
            latitude: 38.7223,
            longitude: -9.1393,
            name: Some("Lisbon".to_string()),
            region: Some("Lisbon".to_string()),
            country: Some("Portugal".to_string()),
            timezone: Some("Europe/Lisbon".to_string()),
        };

        store.set_location(lisbon.clone()).unwrap();
        assert_eq!(store.location().unwrap(), Some(lisbon.clone()));
        assert_eq!(lisbon.label().as_deref(), Some("Lisbon, Portugal"));

        store
            .set_location(Location {
                latitude: -33.87,
                longitude: 151.21,
                ..Location::default()
            })
            .unwrap();
        assert_eq!(
            store.location().unwrap(),
            Some(Location {
                latitude: -33.87,
                longitude: 151.21,
                ..Location::default()
            })
        );
        assert_eq!(store.location().unwrap().unwrap().label(), None);

        store.clear_location().unwrap();
        assert_eq!(store.location().unwrap(), None);
    }

    #[test]
    fn a_location_label_does_not_repeat_its_name() {
        let berlin = Location {
            name: Some("Berlin".to_string()),
            region: Some("Berlin".to_string()),
            country: Some("Germany".to_string()),
            ..Location::default()
        };

        assert_eq!(berlin.label().as_deref(), Some("Berlin, Germany"));
    }

    #[test]
    fn updates_a_profile() {
        let store = Store::open_in_memory().unwrap();
        let id = store.create_profile("Pepper", None, None).unwrap();

        store
            .update_profile(id, "Chilli", Some("Capsicum annuum"), Some("Jalapeño"))
            .unwrap();

        let profile = store.profile(id).unwrap().unwrap();
        assert_eq!(profile.name, "Chilli");
        assert_eq!(profile.species.as_deref(), Some("Capsicum annuum"));
        assert_eq!(profile.cultivar.as_deref(), Some("Jalapeño"));
    }

    fn new_stage(profile_id: i64, position: i64, kind: &str) -> Stage {
        Stage {
            id: 0,
            profile_id,
            position,
            kind: kind.to_string(),
            label: kind.to_string(),
            duration_days_min: None,
            duration_days_max: None,
            notes: None,
            repeats: None,
        }
    }

    fn new_requirement(
        profile_id: i64,
        stage_id: Option<i64>,
        metric: &str,
        day_part: &str,
    ) -> Requirement {
        Requirement {
            id: 0,
            profile_id,
            stage_id,
            metric: metric.to_string(),
            day_part: day_part.to_string(),
            min: Some(10.0),
            max: Some(30.0),
            opt_min: Some(20.0),
            opt_max: Some(26.0),
            survival_min: Some(0.0),
            survival_max: Some(35.0),
        }
    }

    fn new_timing_rule(profile_id: i64) -> TimingRule {
        TimingRule {
            id: 0,
            profile_id,
            stage_id: None,
            kind: "sow_indoor".to_string(),
            anchor: "last_frost".to_string(),
            offset_min_days: Some(-56),
            offset_max_days: Some(-42),
            month_start: None,
            month_end: None,
            day_length_hours: None,
            gdd_base_c: None,
            gdd_target: None,
            repeat_days: None,
            notes: None,
        }
    }

    fn text_attribute(profile_id: i64, key: &str, value: &str) -> Attribute {
        Attribute {
            id: 0,
            profile_id,
            stage_id: None,
            key: key.to_string(),
            value_num: None,
            value_text: Some(value.to_string()),
            value_bool: None,
        }
    }

    fn resolved(
        store: &Store,
        profile_id: i64,
        stage_id: Option<i64>,
        metric: &str,
        day_part: &str,
    ) -> Requirement {
        match store
            .resolved_requirement(profile_id, stage_id, metric, day_part)
            .unwrap()
        {
            Some(RequirementResolution::Resolved(requirement)) => requirement,
            other => panic!("expected a resolved requirement, got {other:?}"),
        }
    }

    fn add_stage_definition(store: &Store, key: &str) {
        store
            .add_custom_attribute_definition(&AttributeDefinition {
                key: key.to_string(),
                label: "Stage attribute".to_string(),
                description: None,
                value_type: "number".to_string(),
                unit: Some("cm".to_string()),
                category: "Other".to_string(),
                applies_to: "stage".to_string(),
                allowed_stages: None,
                allowed_lifecycles: None,
                enum_values: None,
                is_custom: true,
            })
            .unwrap();
    }

    fn confirmed_provenance(entity_type: &str, entity_id: i64) -> Provenance {
        Provenance {
            entity_type: entity_type.to_string(),
            entity_id,
            field: "*".to_string(),
            status: "confirmed".to_string(),
            source_type: None,
            source_ref: None,
            retrieved_at: None,
            confidence: None,
            notes: None,
        }
    }

    #[test]
    fn migrates_the_plant_data_schema() {
        let store = Store::open_in_memory().unwrap();

        let version: i64 = store
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, Store::SCHEMA_VERSION);

        let definitions = store.attribute_definitions().unwrap();
        assert!(definitions.iter().any(|item| item.key == "synonyms"));
        assert!(definitions.iter().any(|item| item.key == "soil_drainage"));
        assert!(definitions.iter().any(|item| item.key == "bbch_range"));
        assert!(
            definitions
                .iter()
                .any(|item| item.key == "soil_drainage" && item.category == "Soil & nutrition")
        );
    }

    #[test]
    fn stores_profile_details() {
        let store = Store::open_in_memory().unwrap();
        let id = store
            .create_profile(
                "Bhut Jolokia",
                Some("Capsicum chinense"),
                Some("Bhut Jolokia"),
            )
            .unwrap();

        let mut details = store.profile_details(id).unwrap().unwrap();
        assert_eq!(details.name, "Bhut Jolokia");
        assert_eq!(details.lifecycle, None);

        details.family = Some("Solanaceae".to_string());
        details.lifecycle = Some("perennial".to_string());
        details.life_form = Some("herbaceous".to_string());
        details.sun_exposure = Some("full_sun".to_string());
        details.photoperiod_type = Some("day_neutral".to_string());
        details.water_need = Some("medium".to_string());
        details.container_suitable = Some(true);
        details.min_container_volume_l_min = Some(10.0);
        details.min_container_volume_l_max = Some(20.0);
        details.days_to_maturity_min = Some(80.0);
        details.days_to_maturity_max = Some(100.0);
        store.update_profile_details(&details).unwrap();

        assert_eq!(store.profile_details(id).unwrap().unwrap(), details);
    }

    #[test]
    fn rejects_inverted_profile_ranges() {
        let store = Store::open_in_memory().unwrap();
        let id = store.create_profile("Tomato", None, None).unwrap();
        let mut details = store.profile_details(id).unwrap().unwrap();

        details.days_to_maturity_min = Some(100.0);
        details.days_to_maturity_max = Some(80.0);
        assert!(store.update_profile_details(&details).is_err());

        details.days_to_maturity_min = Some(80.0);
        details.days_to_maturity_max = Some(100.0);
        details.min_container_volume_l_min = Some(-1.0);
        assert!(store.update_profile_details(&details).is_err());
    }

    #[test]
    fn orders_stages_and_updates_them() {
        let store = Store::open_in_memory().unwrap();
        let id = store.create_profile("Maize", None, None).unwrap();

        store.create_stage(&new_stage(id, 2, "harvest")).unwrap();
        let germination = store
            .create_stage(&new_stage(id, 1, "germination"))
            .unwrap();

        let stages = store.stages(id).unwrap();
        assert_eq!(stages.len(), 2);
        assert_eq!(stages[0].kind, "germination");
        assert_eq!(stages[1].kind, "harvest");

        let mut stage = store.stage(germination).unwrap().unwrap();
        stage.label = "Sprouting".to_string();
        store.update_stage(&stage).unwrap();
        assert_eq!(
            store.stage(germination).unwrap().unwrap().label,
            "Sprouting"
        );

        assert!(store.create_stage(&new_stage(id, 3, "nonsense")).is_err());

        let mut invalid = new_stage(id, 4, "seedling");
        invalid.duration_days_min = Some(20.0);
        invalid.duration_days_max = Some(10.0);
        assert!(store.create_stage(&invalid).is_err());
    }

    #[test]
    fn a_stage_repeat_marker_round_trips_and_stays_null_when_unset() {
        let store = Store::open_in_memory().unwrap();
        let id = store.create_profile("Rose", None, None).unwrap();

        let mut stage = new_stage(id, 1, "vegetative");
        stage.repeats = Some(false);
        let stage_id = store.create_stage(&stage).unwrap();
        assert_eq!(store.stage(stage_id).unwrap().unwrap().repeats, Some(false));

        stage.id = stage_id;
        stage.repeats = Some(true);
        store.update_stage(&stage).unwrap();
        assert_eq!(store.stage(stage_id).unwrap().unwrap().repeats, Some(true));

        stage.repeats = None;
        store.update_stage(&stage).unwrap();
        assert_eq!(store.stage(stage_id).unwrap().unwrap().repeats, None);
    }

    #[test]
    fn moves_and_reorders_stages() {
        let store = Store::open_in_memory().unwrap();
        let id = store.create_profile("Maize", None, None).unwrap();
        let germination = store
            .create_stage(&new_stage(id, 1, "germination"))
            .unwrap();
        let harvest = store.create_stage(&new_stage(id, 2, "harvest")).unwrap();
        let ripening = store.create_stage(&new_stage(id, 3, "ripening")).unwrap();

        store.move_stage(ripening, -1).unwrap();
        let kinds: Vec<String> = store
            .stages(id)
            .unwrap()
            .into_iter()
            .map(|stage| stage.kind)
            .collect();
        assert_eq!(kinds, ["germination", "ripening", "harvest"]);

        store
            .reorder_stages(id, &[harvest, germination, ripening])
            .unwrap();
        let ids: Vec<i64> = store
            .stages(id)
            .unwrap()
            .into_iter()
            .map(|stage| stage.id)
            .collect();
        assert_eq!(ids, [harvest, germination, ripening]);
    }

    #[test]
    fn migrates_a_v7_database_and_backfills_categories() {
        let path = std::env::temp_dir().join(format!(
            "omagrow-v7-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_file(&path);

        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE attribute_definitions (
                     key                TEXT PRIMARY KEY,
                     label              TEXT NOT NULL,
                     description        TEXT,
                     value_type         TEXT NOT NULL,
                     unit               TEXT,
                     applies_to         TEXT NOT NULL DEFAULT 'profile',
                     allowed_stages     TEXT,
                     allowed_lifecycles TEXT,
                     enum_values        TEXT,
                     is_custom          INTEGER NOT NULL DEFAULT 0,
                     created_at         TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
                 );
                 INSERT INTO attribute_definitions
                     (key, label, value_type, is_custom, created_at)
                 VALUES ('synonyms', 'Synonyms', 'text', 0, '2026-01-01T00:00:00Z');
                 INSERT INTO attribute_definitions
                     (key, label, value_type, is_custom, created_at)
                 VALUES ('pot_width_cm', 'Pot width', 'number', 1, '2026-01-01T00:00:00Z');
                 CREATE TABLE locations (
                     id         INTEGER PRIMARY KEY CHECK (id = 1),
                     latitude   REAL NOT NULL,
                     longitude  REAL NOT NULL,
                     updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
                 );
                 INSERT INTO locations (id, latitude, longitude) VALUES (1, 38.72, -9.14);
                 CREATE TABLE profile_stages (
                     id                INTEGER PRIMARY KEY AUTOINCREMENT,
                     profile_id        INTEGER NOT NULL,
                     position          INTEGER NOT NULL,
                     kind              TEXT NOT NULL,
                     label             TEXT NOT NULL,
                     duration_days_min REAL,
                     duration_days_max REAL,
                     notes             TEXT
                 );
                 PRAGMA user_version = 7;",
            )
            .unwrap();
        }

        let store = Store::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), Store::SCHEMA_VERSION);

        let synonyms = store.attribute_definition("synonyms").unwrap().unwrap();
        assert_eq!(synonyms.category, "About");
        let custom = store.attribute_definition("pot_width_cm").unwrap().unwrap();
        assert_eq!(custom.category, "Other");

        // A location from before the place columns keeps its coordinates and gains empty fields.
        let location = store.location().unwrap().unwrap();
        assert_eq!(location.latitude, 38.72);
        assert_eq!(location.longitude, -9.14);
        assert_eq!(location.name, None);
        assert_eq!(location.timezone, None);

        drop(store);
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(path.with_extension("db-wal"));
        let _ = fs::remove_file(path.with_extension("db-shm"));
    }

    #[test]
    fn resolves_requirements_with_stage_overrides() {
        let store = Store::open_in_memory().unwrap();
        let id = store.create_profile("Chilli", None, None).unwrap();
        let flowering = store.create_stage(&new_stage(id, 1, "flowering")).unwrap();
        let fruiting = store.create_stage(&new_stage(id, 2, "fruiting")).unwrap();

        let base_day = store
            .set_requirement(&new_requirement(id, None, "air_temp", "day"))
            .unwrap();
        let base_night = store
            .set_requirement(&new_requirement(id, None, "air_temp", "night"))
            .unwrap();
        let mut stage_day = new_requirement(id, Some(flowering), "air_temp", "day");
        stage_day.min = Some(18.0);
        stage_day.max = Some(28.0);
        stage_day.opt_min = Some(20.0);
        stage_day.opt_max = Some(25.0);
        stage_day.survival_min = Some(10.0);
        stage_day.survival_max = Some(30.0);
        let stage_day_id = store.set_requirement(&stage_day).unwrap();

        assert_eq!(store.requirements(id).unwrap().len(), 3);
        assert_eq!(
            resolved(&store, id, Some(flowering), "air_temp", "day").id,
            stage_day_id
        );
        assert_eq!(
            resolved(&store, id, Some(flowering), "air_temp", "night").id,
            base_night
        );
        assert_eq!(
            resolved(&store, id, Some(fruiting), "air_temp", "day").id,
            base_day
        );

        let mut updated = new_requirement(id, None, "air_temp", "day");
        updated.max = Some(32.0);
        store.set_requirement(&updated).unwrap();
        assert_eq!(store.requirements(id).unwrap().len(), 3);
        assert_eq!(
            resolved(&store, id, Some(fruiting), "air_temp", "day").max,
            Some(32.0)
        );

        let mut inverted = new_requirement(id, None, "air_temp", "day");
        inverted.opt_min = Some(31.0);
        inverted.max = Some(30.0);
        assert!(store.set_requirement(&inverted).is_err());

        let other = store.create_profile("Basil", None, None).unwrap();
        let other_stage = store
            .create_stage(&new_stage(other, 1, "vegetative"))
            .unwrap();
        let mut wrong_stage = new_requirement(id, Some(other_stage), "air_temp", "day");
        wrong_stage.metric = "rh".to_string();
        assert!(store.set_requirement(&wrong_stage).is_err());
    }

    #[test]
    fn rejects_empty_requirements_and_out_of_bounds_metrics() {
        let store = Store::open_in_memory().unwrap();
        let id = store.create_profile("Basil", None, None).unwrap();

        let mut empty = new_requirement(id, None, "air_temp", "day");
        empty.min = None;
        empty.max = None;
        empty.opt_min = None;
        empty.opt_max = None;
        empty.survival_min = None;
        empty.survival_max = None;
        assert!(store.set_requirement(&empty).is_err());

        for (metric, value) in [
            ("rh", 150.0),
            ("soil_ph", 15.0),
            ("photoperiod", 25.0),
            ("air_temp", 100.0),
        ] {
            let mut requirement = new_requirement(id, None, metric, "any");
            requirement.min = Some(value);
            requirement.max = None;
            requirement.opt_min = None;
            requirement.opt_max = None;
            requirement.survival_min = None;
            requirement.survival_max = None;

            assert!(
                store.set_requirement(&requirement).is_err(),
                "{metric} accepted {value}"
            );
        }
    }

    #[test]
    fn resolves_any_day_part_and_rejects_a_foreign_stage() {
        let store = Store::open_in_memory().unwrap();
        let id = store.create_profile("Chilli", None, None).unwrap();
        let flowering = store.create_stage(&new_stage(id, 1, "flowering")).unwrap();

        let mut any = new_requirement(id, Some(flowering), "air_temp", "any");
        any.min = Some(16.0);
        any.max = Some(28.0);
        store.set_requirement(&any).unwrap();

        let day = resolved(&store, id, Some(flowering), "air_temp", "day");
        assert_eq!(day.day_part, "any");
        assert_eq!(day.stage_id, Some(flowering));

        let night = resolved(&store, id, Some(flowering), "air_temp", "night");
        assert_eq!(night.day_part, "any");

        let other = store.create_profile("Basil", None, None).unwrap();
        let other_stage = store
            .create_stage(&new_stage(other, 1, "vegetative"))
            .unwrap();
        assert!(
            store
                .resolved_requirement(id, Some(other_stage), "air_temp", "day")
                .is_err()
        );
    }

    #[test]
    fn treats_not_applicable_as_an_override() {
        let store = Store::open_in_memory().unwrap();
        let id = store.create_profile("Chilli", None, None).unwrap();
        let flowering = store.create_stage(&new_stage(id, 1, "flowering")).unwrap();
        let fruiting = store.create_stage(&new_stage(id, 2, "fruiting")).unwrap();

        let base = store
            .set_requirement(&new_requirement(id, None, "air_temp", "day"))
            .unwrap();
        assert_eq!(
            resolved(&store, id, Some(flowering), "air_temp", "day").id,
            base
        );

        let stage_row = store
            .set_requirement(&new_requirement(id, Some(flowering), "air_temp", "day"))
            .unwrap();
        let mut stage_provenance = confirmed_provenance("requirement", stage_row);
        stage_provenance.status = "not_applicable".to_string();
        store.set_provenance(&stage_provenance).unwrap();

        assert_eq!(
            store
                .resolved_requirement(id, Some(flowering), "air_temp", "day")
                .unwrap(),
            Some(RequirementResolution::NotApplicable)
        );
        assert_eq!(
            resolved(&store, id, Some(fruiting), "air_temp", "day").id,
            base
        );

        let any_row = store
            .set_requirement(&new_requirement(id, Some(fruiting), "rh", "any"))
            .unwrap();
        let mut any_provenance = confirmed_provenance("requirement", any_row);
        any_provenance.status = "not_applicable".to_string();
        store.set_provenance(&any_provenance).unwrap();

        assert_eq!(
            store
                .resolved_requirement(id, Some(fruiting), "rh", "day")
                .unwrap(),
            Some(RequirementResolution::NotApplicable)
        );

        let night_row = store
            .set_requirement(&new_requirement(id, Some(flowering), "rh", "night"))
            .unwrap();
        let flowering_any = store
            .set_requirement(&new_requirement(id, Some(flowering), "rh", "any"))
            .unwrap();
        let mut flowering_any_provenance = confirmed_provenance("requirement", flowering_any);
        flowering_any_provenance.status = "not_applicable".to_string();
        store.set_provenance(&flowering_any_provenance).unwrap();

        assert_eq!(
            store
                .resolved_requirement(id, Some(flowering), "rh", "day")
                .unwrap(),
            Some(RequirementResolution::NotApplicable)
        );
        assert_eq!(
            resolved(&store, id, Some(flowering), "rh", "night").id,
            night_row
        );

        let mut night_provenance = confirmed_provenance("requirement", night_row);
        night_provenance.status = "not_applicable".to_string();
        store.set_provenance(&night_provenance).unwrap();

        assert_eq!(
            store
                .resolved_requirement(id, Some(flowering), "rh", "night")
                .unwrap(),
            Some(RequirementResolution::NotApplicable)
        );

        let mut default_provenance = confirmed_provenance("requirement", base);
        default_provenance.status = "not_applicable".to_string();
        store.set_provenance(&default_provenance).unwrap();

        assert_eq!(
            store
                .resolved_requirement(id, None, "air_temp", "day")
                .unwrap(),
            Some(RequirementResolution::NotApplicable)
        );
        assert_eq!(
            store
                .resolved_requirement(id, Some(fruiting), "air_temp", "day")
                .unwrap(),
            Some(RequirementResolution::NotApplicable)
        );
    }

    #[test]
    fn rejects_empty_profile_names() {
        let store = Store::open_in_memory().unwrap();
        assert!(store.create_profile("  ", None, None).is_err());

        let id = store.create_profile("Basil", None, None).unwrap();
        assert!(store.update_profile(id, " ", None, None).is_err());
    }

    #[test]
    fn stores_and_updates_timing_rules() {
        let store = Store::open_in_memory().unwrap();
        let id = store.create_profile("Tomato", None, None).unwrap();

        let mut rule = new_timing_rule(id);
        let rule_id = store.create_timing_rule(&rule).unwrap();
        let rules = store.timing_rules(id).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].offset_min_days, Some(-56));
        assert_eq!(rules[0].offset_max_days, Some(-42));

        rule.id = rule_id;
        rule.notes = Some("Start under cover.".to_string());
        store.update_timing_rule(&rule).unwrap();
        assert_eq!(
            store.timing_rules(id).unwrap()[0].notes.as_deref(),
            Some("Start under cover.")
        );

        let mut calendar = new_timing_rule(id);
        calendar.kind = "repot".to_string();
        calendar.anchor = "calendar".to_string();
        calendar.month_start = None;
        calendar.month_end = None;
        assert!(store.create_timing_rule(&calendar).is_err());

        let mut day_length = new_timing_rule(id);
        day_length.kind = "flowering".to_string();
        day_length.anchor = "day_length".to_string();
        day_length.day_length_hours = None;
        assert!(store.create_timing_rule(&day_length).is_err());

        let mut inverted = new_timing_rule(id);
        inverted.offset_min_days = Some(10);
        inverted.offset_max_days = Some(5);
        assert!(store.create_timing_rule(&inverted).is_err());

        store.delete_timing_rule(rule_id).unwrap();
        assert!(store.timing_rules(id).unwrap().is_empty());
    }

    #[test]
    fn seeds_and_stores_attributes() {
        let store = Store::open_in_memory().unwrap();
        let id = store.create_profile("Chilli", None, None).unwrap();

        let synonyms = store.attribute_definition("synonyms").unwrap().unwrap();
        assert_eq!(synonyms.value_type, "text");
        assert!(!synonyms.is_custom);

        let attribute = text_attribute(id, "synonyms", "Ghost pepper");
        let attribute_id = store.set_attribute(&attribute).unwrap();
        let attributes = store.attributes(id).unwrap();
        assert_eq!(attributes.len(), 1);
        assert_eq!(attributes[0].value_text.as_deref(), Some("Ghost pepper"));

        let updated = text_attribute(id, "synonyms", "Bhut Jolokia");
        store.set_attribute(&updated).unwrap();
        assert_eq!(store.attributes(id).unwrap().len(), 1);
        assert_eq!(
            store.attributes(id).unwrap()[0].value_text.as_deref(),
            Some("Bhut Jolokia")
        );

        let mut wrong_type = text_attribute(id, "synonyms", "ignored");
        wrong_type.value_text = None;
        wrong_type.value_num = Some(3.0);
        assert!(store.set_attribute(&wrong_type).is_err());

        let mut empty = text_attribute(id, "synonyms", "   ");
        empty.value_text = Some("   ".to_string());
        assert!(store.set_attribute(&empty).is_err());

        let mut drainage = text_attribute(id, "soil_drainage", "free");
        store.set_attribute(&drainage).unwrap();
        drainage.value_text = Some("boggy".to_string());
        assert!(store.set_attribute(&drainage).is_err());

        store
            .add_custom_attribute_definition(&AttributeDefinition {
                key: "pot_width_cm".to_string(),
                label: "Pot width".to_string(),
                description: Some("Width of the container used for this profile.".to_string()),
                value_type: "number".to_string(),
                unit: Some("cm".to_string()),
                category: "Other".to_string(),
                applies_to: "profile".to_string(),
                allowed_stages: None,
                allowed_lifecycles: None,
                enum_values: None,
                is_custom: true,
            })
            .unwrap();
        let definition = store.attribute_definition("pot_width_cm").unwrap().unwrap();
        assert!(definition.is_custom);

        let custom = Attribute {
            id: 0,
            profile_id: id,
            stage_id: None,
            key: "pot_width_cm".to_string(),
            value_num: Some(30.0),
            value_text: None,
            value_bool: None,
        };
        store.set_attribute(&custom).unwrap();
        assert!(
            store
                .attributes(id)
                .unwrap()
                .iter()
                .any(|item| item.key == "pot_width_cm" && item.value_num == Some(30.0))
        );

        let unknown = text_attribute(id, "does_not_exist", "value");
        assert!(store.set_attribute(&unknown).is_err());

        let stage = store.create_stage(&new_stage(id, 1, "flowering")).unwrap();
        let mut profile_key_on_stage = text_attribute(id, "synonyms", "ignored");
        profile_key_on_stage.stage_id = Some(stage);
        assert!(store.set_attribute(&profile_key_on_stage).is_err());

        add_stage_definition(&store, "node_spacing_cm");

        let mut stage_only = Attribute {
            id: 0,
            profile_id: id,
            stage_id: None,
            key: "node_spacing_cm".to_string(),
            value_num: Some(4.0),
            value_text: None,
            value_bool: None,
        };
        assert!(store.set_attribute(&stage_only).is_err());

        stage_only.stage_id = Some(stage);
        store.set_attribute(&stage_only).unwrap();

        store.delete_attribute(attribute_id).unwrap();
        assert!(
            !store
                .attributes(id)
                .unwrap()
                .iter()
                .any(|item| item.key == "synonyms")
        );
    }

    #[test]
    fn seeding_again_writes_nothing() {
        let store = Store::open_in_memory().unwrap();
        let before = store.conn.total_changes();
        store.seed_attribute_definitions().unwrap();
        assert_eq!(store.conn.total_changes(), before);
    }

    #[test]
    fn rejects_duplicate_custom_keys() {
        let store = Store::open_in_memory().unwrap();
        let definition = AttributeDefinition {
            key: "pot_width_cm".to_string(),
            label: "Pot width".to_string(),
            description: None,
            value_type: "number".to_string(),
            unit: Some("cm".to_string()),
            category: "Other".to_string(),
            applies_to: "profile".to_string(),
            allowed_stages: None,
            allowed_lifecycles: None,
            enum_values: None,
            is_custom: true,
        };

        store.add_custom_attribute_definition(&definition).unwrap();
        assert!(store.add_custom_attribute_definition(&definition).is_err());
    }

    #[test]
    fn rejects_an_unknown_attribute_category() {
        let store = Store::open_in_memory().unwrap();
        let definition = AttributeDefinition {
            key: "pot_width_cm".to_string(),
            label: "Pot width".to_string(),
            description: None,
            value_type: "number".to_string(),
            unit: Some("cm".to_string()),
            category: "Nonsense".to_string(),
            applies_to: "profile".to_string(),
            allowed_stages: None,
            allowed_lifecycles: None,
            enum_values: None,
            is_custom: true,
        };

        assert!(store.add_custom_attribute_definition(&definition).is_err());
    }

    #[test]
    fn refreshes_seeded_definitions() {
        let store = Store::open_in_memory().unwrap();
        store
            .conn
            .execute(
                "UPDATE attribute_definitions SET label = 'Changed' WHERE key = 'synonyms'",
                [],
            )
            .unwrap();

        store.seed_attribute_definitions().unwrap();

        assert_eq!(
            store
                .attribute_definition("synonyms")
                .unwrap()
                .unwrap()
                .label,
            "Synonyms"
        );
    }

    #[test]
    fn stores_provenance() {
        let store = Store::open_in_memory().unwrap();
        let id = store.create_profile("Apple", None, None).unwrap();

        let mut provenance = confirmed_provenance("profile", id);
        provenance.status = "estimated".to_string();
        provenance.source_type = None;
        assert!(store.set_provenance(&provenance).is_err());

        provenance.source_type = Some("agent".to_string());
        provenance.source_ref = Some("RHS plant selector".to_string());
        provenance.retrieved_at = Some("2026-10-28".to_string());
        provenance.confidence = Some("medium".to_string());
        store.set_provenance(&provenance).unwrap();

        let stored = store.provenance("profile", id, "*").unwrap().unwrap();
        assert_eq!(stored.status, "estimated");
        assert_eq!(stored.source_ref.as_deref(), Some("RHS plant selector"));

        provenance.status = "confirmed".to_string();
        store.set_provenance(&provenance).unwrap();
        assert_eq!(
            store
                .provenance("profile", id, "*")
                .unwrap()
                .unwrap()
                .status,
            "confirmed"
        );

        let mut invalid = confirmed_provenance("profile", id);
        invalid.status = "guessed".to_string();
        assert!(store.set_provenance(&invalid).is_err());

        store.delete_provenance("profile", id, "*").unwrap();
        assert_eq!(store.provenance("profile", id, "*").unwrap(), None);
    }

    #[test]
    fn copies_a_profile_with_its_plant_data_and_provenance() {
        let store = Store::open_in_memory().unwrap();
        let source = store
            .create_profile(
                "Ghost Pepper",
                Some("Capsicum chinense"),
                Some("Bhut Jolokia"),
            )
            .unwrap();
        let mut details = store.profile_details(source).unwrap().unwrap();
        details.family = Some("Solanaceae".to_string());
        details.lifecycle = Some("perennial".to_string());
        store.update_profile_details(&details).unwrap();

        let stage = store
            .create_stage(&new_stage(source, 1, "ripening"))
            .unwrap();
        store
            .set_requirement(&new_requirement(source, Some(stage), "air_temp", "day"))
            .unwrap();
        store.create_timing_rule(&new_timing_rule(source)).unwrap();
        let attribute = store
            .set_attribute(&Attribute {
                id: 0,
                profile_id: source,
                stage_id: None,
                key: "climate_type".to_string(),
                value_num: None,
                value_text: Some("tropical".to_string()),
                value_bool: None,
            })
            .unwrap();
        store
            .set_provenance(&Provenance {
                entity_type: "attribute".to_string(),
                entity_id: attribute,
                field: "*".to_string(),
                status: "estimated".to_string(),
                source_type: Some("website".to_string()),
                source_ref: Some("https://example.org".to_string()),
                retrieved_at: None,
                confidence: Some("high".to_string()),
                notes: None,
            })
            .unwrap();

        let copy = store.copy_profile(source).unwrap();
        assert_ne!(copy, source);
        let copied = store.profile(copy).unwrap().unwrap();
        assert_eq!(copied.name, "Ghost Pepper (copy)");
        assert_eq!(copied.species.as_deref(), Some("Capsicum chinense"));

        let copied_details = store.profile_details(copy).unwrap().unwrap();
        assert_eq!(copied_details.family.as_deref(), Some("Solanaceae"));
        assert_eq!(copied_details.lifecycle.as_deref(), Some("perennial"));

        let copied_stages = store.stages(copy).unwrap();
        assert_eq!(copied_stages.len(), 1);
        let copied_requirements = store.requirements(copy).unwrap();
        assert_eq!(copied_requirements.len(), 1);
        assert_eq!(copied_requirements[0].stage_id, Some(copied_stages[0].id));
        assert_eq!(store.timing_rules(copy).unwrap().len(), 1);
        assert_eq!(store.attributes(copy).unwrap().len(), 1);
        assert_eq!(
            store
                .provenance("attribute", store.attributes(copy).unwrap()[0].id, "*")
                .unwrap()
                .unwrap()
                .source_ref
                .as_deref(),
            Some("https://example.org")
        );

        assert_eq!(store.stages(source).unwrap().len(), 1);
        assert_eq!(store.attributes(source).unwrap().len(), 1);

        let named = store
            .copy_profile_named(source, Some("Named copy"))
            .unwrap();
        assert_eq!(store.profile(named).unwrap().unwrap().name, "Named copy");
        assert!(store.copy_profile_named(source, Some("  ")).is_err());
    }

    #[test]
    fn deletes_profile_with_children_and_provenance() {
        let store = Store::open_in_memory().unwrap();
        let id = store.create_profile("Maize", None, None).unwrap();
        let stage_id = store.create_stage(&new_stage(id, 1, "vegetative")).unwrap();
        let requirement_id = store
            .set_requirement(&new_requirement(id, Some(stage_id), "air_temp", "day"))
            .unwrap();
        let rule_id = store.create_timing_rule(&new_timing_rule(id)).unwrap();
        let attribute_id = store
            .set_attribute(&text_attribute(id, "synonyms", "Corn"))
            .unwrap();

        for (entity_type, entity_id) in [
            ("profile", id),
            ("stage", stage_id),
            ("requirement", requirement_id),
            ("timing_rule", rule_id),
            ("attribute", attribute_id),
        ] {
            store
                .set_provenance(&confirmed_provenance(entity_type, entity_id))
                .unwrap();
        }

        store.delete_profile(id).unwrap();

        assert_eq!(store.profile(id).unwrap(), None);
        assert!(store.stages(id).unwrap().is_empty());
        assert!(store.requirements(id).unwrap().is_empty());
        assert!(store.timing_rules(id).unwrap().is_empty());
        assert!(store.attributes(id).unwrap().is_empty());
        assert_eq!(store.provenance("profile", id, "*").unwrap(), None);
        assert_eq!(store.provenance("stage", stage_id, "*").unwrap(), None);
        assert_eq!(
            store
                .provenance("requirement", requirement_id, "*")
                .unwrap(),
            None
        );
        assert_eq!(store.provenance("timing_rule", rule_id, "*").unwrap(), None);
        assert_eq!(
            store.provenance("attribute", attribute_id, "*").unwrap(),
            None
        );
    }

    #[test]
    fn stores_and_clears_an_avatar_icon() {
        let store = Store::open_in_memory().unwrap();
        let profile = store.create_profile("Basil", None, None).unwrap();

        assert_eq!(store.avatar_icon("profile", profile).unwrap(), None);
        store
            .set_avatar_icon("profile", profile, Some("omagrow-plant-symbolic"))
            .unwrap();
        assert_eq!(
            store.avatar_icon("profile", profile).unwrap().as_deref(),
            Some("omagrow-plant-symbolic")
        );

        store.set_avatar_icon("profile", profile, None).unwrap();
        assert_eq!(store.avatar_icon("profile", profile).unwrap(), None);

        store
            .set_avatar_icon("profile", profile, Some("crop:tomato"))
            .unwrap();
        assert_eq!(
            store.avatar_icon("profile", profile).unwrap().as_deref(),
            Some("crop:tomato")
        );
        store.set_avatar_icon("profile", profile, None).unwrap();

        assert!(
            store
                .set_avatar_icon("profile", profile, Some("../outside"))
                .is_err()
        );
        assert!(
            store
                .set_avatar_icon("garden", profile, Some("omagrow-plant-symbolic"))
                .is_err()
        );
    }

    #[test]
    fn deleting_an_owner_removes_its_avatar_icon() {
        let store = Store::open_in_memory().unwrap();
        let profile = store.create_profile("Basil", None, None).unwrap();
        store
            .set_avatar_icon("profile", profile, Some("omagrow-plant-symbolic"))
            .unwrap();

        let space = store.create_space(&space_input(Some("Tent"))).unwrap();
        let planting = store
            .create_planting(&planting_input(profile, space))
            .unwrap();
        store
            .set_avatar_icon("planting", planting, Some("omagrow-planting-symbolic"))
            .unwrap();

        store.delete_planting(planting).unwrap();
        assert_eq!(store.avatar_icon("planting", planting).unwrap(), None);

        let copy = store.create_profile("Chives", None, None).unwrap();
        store
            .set_avatar_icon("profile", copy, Some("omagrow-plant-symbolic"))
            .unwrap();
        store.delete_profile(copy).unwrap();
        assert_eq!(store.avatar_icon("profile", copy).unwrap(), None);

        store.delete_profile(profile).unwrap();
        assert_eq!(store.avatar_icon("profile", profile).unwrap(), None);
    }

    #[test]
    fn stores_one_image_per_owner_and_role() {
        let store = Store::open_in_memory().unwrap();
        let id = store.create_profile("Chilli", None, None).unwrap();
        let media = Media {
            id: 0,
            owner_type: "profile".to_string(),
            owner_id: id,
            role: "cover".to_string(),
            file_stem: "profile-1-cover".to_string(),
            width: 1600,
            height: 900,
            bytes: 120_000,
        };

        assert_eq!(store.media("profile", id, "cover").unwrap(), None);
        let media_id = store.set_media(&media).unwrap();

        let stored = store.media("profile", id, "cover").unwrap().unwrap();
        assert_eq!(stored.file_stem, "profile-1-cover");
        assert_eq!(stored.width, 1600);

        let updated = Media {
            width: 800,
            height: 600,
            ..media.clone()
        };
        assert_eq!(store.set_media(&updated).unwrap(), media_id);
        assert_eq!(store.media_for("profile", id).unwrap().len(), 1);
        assert_eq!(store.media_for("profile", id).unwrap()[0].width, 800);

        let mut invalid = media.clone();
        invalid.width = 0;
        assert!(store.set_media(&invalid).is_err());

        let mut traversing = media.clone();
        traversing.file_stem = "../outside".to_string();
        assert!(store.set_media(&traversing).is_err());

        store.delete_media("profile", id, "cover").unwrap();
        assert_eq!(store.media("profile", id, "cover").unwrap(), None);

        store.set_media(&media).unwrap();
        store.delete_profile(id).unwrap();
        assert!(store.media_for("profile", id).unwrap().is_empty());
    }

    #[test]
    fn refuses_to_delete_a_stage_that_still_has_values() {
        let store = Store::open_in_memory().unwrap();
        let id = store.create_profile("Chilli", None, None).unwrap();
        let stage_id = store.create_stage(&new_stage(id, 1, "flowering")).unwrap();
        let requirement_id = store
            .set_requirement(&new_requirement(id, Some(stage_id), "rh", "day"))
            .unwrap();
        add_stage_definition(&store, "node_spacing_cm");
        let attribute_id = store
            .set_attribute(&Attribute {
                id: 0,
                profile_id: id,
                stage_id: Some(stage_id),
                key: "node_spacing_cm".to_string(),
                value_num: Some(4.0),
                value_text: None,
                value_bool: None,
            })
            .unwrap();
        let mut rule = new_timing_rule(id);
        rule.stage_id = Some(stage_id);
        let rule_id = store.create_timing_rule(&rule).unwrap();

        for (entity_type, entity_id) in [
            ("stage", stage_id),
            ("requirement", requirement_id),
            ("attribute", attribute_id),
            ("timing_rule", rule_id),
        ] {
            store
                .set_provenance(&confirmed_provenance(entity_type, entity_id))
                .unwrap();
        }

        assert!(store.delete_stage(stage_id).is_err());
        assert!(store.stage(stage_id).unwrap().is_some());
        assert_eq!(store.requirements(id).unwrap().len(), 1);

        store.delete_requirement(requirement_id).unwrap();
        store.delete_attribute(attribute_id).unwrap();
        store.delete_timing_rule(rule_id).unwrap();
        store.delete_stage(stage_id).unwrap();

        assert_eq!(store.stage(stage_id).unwrap(), None);
        assert!(store.requirements(id).unwrap().is_empty());
        assert!(store.attributes(id).unwrap().is_empty());
        assert!(store.timing_rules(id).unwrap().is_empty());
        assert_eq!(store.provenance("stage", stage_id, "*").unwrap(), None);
        assert_eq!(
            store
                .provenance("requirement", requirement_id, "*")
                .unwrap(),
            None
        );
        assert_eq!(
            store.provenance("attribute", attribute_id, "*").unwrap(),
            None
        );
        assert_eq!(store.provenance("timing_rule", rule_id, "*").unwrap(), None);
    }

    #[test]
    fn deletes_a_requirement_and_its_provenance() {
        let store = Store::open_in_memory().unwrap();
        let id = store.create_profile("Basil", None, None).unwrap();
        let requirement_id = store
            .set_requirement(&new_requirement(id, None, "rh", "day"))
            .unwrap();
        store
            .set_provenance(&confirmed_provenance("requirement", requirement_id))
            .unwrap();

        store.delete_requirement(requirement_id).unwrap();

        assert!(store.requirements(id).unwrap().is_empty());
        assert_eq!(
            store
                .provenance("requirement", requirement_id, "*")
                .unwrap(),
            None
        );
    }

    fn space_input(name: Option<&str>) -> SpaceInput {
        SpaceInput {
            name: name.map(str::to_string),
            kind: "indoor".into(),
            light_type: Some("led".into()),
            light_hours: Some(12.0),
            light_start: Some(6 * 60),
            notes: None,
        }
    }

    fn planting_input(profile_id: i64, space_id: i64) -> PlantingInput {
        PlantingInput {
            profile_id,
            space_id,
            name: None,
            quantity: 5,
            container: "pot".into(),
            container_volume_l: Some(3.0),
            substrate: Some("Coco and perlite".into()),
            started_on: crate::calendar::Date::new(2026, 10, 30).unwrap(),
            start_kind: "planted".into(),
            moved_on: None,
            notes: None,
        }
    }

    #[test]
    fn migrates_the_planting_tables() {
        let store = Store::open_in_memory().unwrap();

        assert_eq!(store.schema_version().unwrap(), Store::SCHEMA_VERSION);
        assert!(store.spaces().unwrap().is_empty());
        assert!(store.plantings().unwrap().is_empty());
    }

    #[test]
    fn creates_a_space_and_a_planting() {
        let store = Store::open_in_memory().unwrap();
        let profile = store
            .create_profile("Northern Light", Some("Cannabis sativa"), None)
            .unwrap();
        let space = store.create_space(&space_input(Some("Grow tent"))).unwrap();
        let id = store
            .create_planting(&planting_input(profile, space))
            .unwrap();

        let summary = store.planting(id).unwrap().unwrap();
        assert_eq!(summary.profile_name, "Northern Light");
        assert_eq!(summary.profile_species.as_deref(), Some("Cannabis sativa"));
        assert_eq!(summary.space_name.as_deref(), Some("Grow tent"));
        assert_eq!(summary.planting.quantity, 5);
        assert_eq!(summary.planting.container_volume_l, Some(3.0));
        assert_eq!(summary.title(), "Northern Light");

        let today = crate::calendar::Date::new(2026, 10, 30).unwrap();
        assert_eq!(
            summary.planting.state(today),
            crate::planting::PlantingState::Growing
        );
        assert_eq!(summary.planting.day_number(today), 1);
    }

    #[test]
    fn a_standalone_planting_gets_a_private_space() {
        let store = Store::open_in_memory().unwrap();
        let profile = store.create_profile("Monstera", None, None).unwrap();
        let space = store.create_space(&space_input(None)).unwrap();
        let id = store
            .create_planting(&planting_input(profile, space))
            .unwrap();

        assert!(store.spaces().unwrap().is_empty());
        assert_eq!(store.space(space).unwrap().unwrap().name, None);
        assert_eq!(store.planting(id).unwrap().unwrap().space_name, None);
    }

    #[test]
    fn rejects_incomplete_plantings() {
        let store = Store::open_in_memory().unwrap();
        let profile = store.create_profile("Maize", None, None).unwrap();
        let space = store.create_space(&space_input(Some("Field"))).unwrap();

        let mut input = planting_input(profile, space);
        input.quantity = 0;
        assert!(store.create_planting(&input).is_err());

        let mut input = planting_input(profile, space);
        input.container = "bucket".into();
        assert!(store.create_planting(&input).is_err());

        let mut input = planting_input(profile, space);
        input.start_kind = "thrown".into();
        assert!(store.create_planting(&input).is_err());

        assert!(store.create_planting(&planting_input(999, space)).is_err());
        assert!(
            store
                .create_planting(&planting_input(profile, 999))
                .is_err()
        );
    }

    #[test]
    fn a_profile_with_plantings_cannot_be_deleted() {
        let store = Store::open_in_memory().unwrap();
        let profile = store.create_profile("Apple", None, None).unwrap();
        let space = store
            .create_space(&space_input(Some("Back garden")))
            .unwrap();
        let planting = store
            .create_planting(&planting_input(profile, space))
            .unwrap();

        let error = store.delete_profile(profile).unwrap_err().to_string();
        assert!(error.contains("plantings"));
        assert!(store.profile(profile).unwrap().is_some());

        store.delete_planting(planting).unwrap();
        store.delete_profile(profile).unwrap();
        assert!(store.profile(profile).unwrap().is_none());
    }

    #[test]
    fn moving_out_of_a_private_space_keeps_its_history() {
        let store = Store::open_in_memory().unwrap();
        let profile = store.create_profile("Basil", None, None).unwrap();
        let private = store.create_space(&space_input(None)).unwrap();
        let shared = store.create_space(&space_input(Some("Kitchen"))).unwrap();
        let id = store
            .create_planting(&planting_input(profile, private))
            .unwrap();
        store
            .create_climate_reading(&reading_input(private, 30, Some("09:00"), Some(20.0), None))
            .unwrap();

        store
            .update_planting(id, &planting_input(profile, shared))
            .unwrap();

        // The private space is no longer listed as a place, but its placement keeps the
        // measurements reachable from the planting.
        assert!(store.space(private).unwrap().is_some());
        assert_eq!(store.planting_climate_readings(id).unwrap().len(), 1);
        assert_eq!(
            store.planting(id).unwrap().unwrap().space_name.as_deref(),
            Some("Kitchen")
        );
        assert_eq!(store.planting_placements(id).unwrap().len(), 2);
        assert_eq!(store.spaces().unwrap().len(), 1);
    }

    #[test]
    fn creates_a_placement_for_each_planting() {
        let store = Store::open_in_memory().unwrap();
        let profile = store.create_profile("Basil", None, None).unwrap();
        let space = store.create_space(&space_input(Some("Kitchen"))).unwrap();
        let id = store
            .create_planting(&planting_input(profile, space))
            .unwrap();

        let placements = store.planting_placements(id).unwrap();
        assert_eq!(placements.len(), 1);
        assert_eq!(placements[0].space_id, space);
        assert_eq!(placements[0].space_kind, "indoor");
        assert_eq!(
            placements[0].started_on,
            crate::calendar::Date::new(2026, 10, 30).unwrap()
        );
        assert_eq!(placements[0].ended_on, None);
    }

    #[test]
    fn finishing_before_the_current_placement_is_refused() {
        let store = Store::open_in_memory().unwrap();
        let profile = store.create_profile("Basil", None, None).unwrap();
        let first = store.create_space(&space_input(Some("Kitchen"))).unwrap();
        let second = store
            .create_space(&space_input(Some("Greenhouse")))
            .unwrap();
        let id = store
            .create_planting(&planting_input(profile, first))
            .unwrap();
        let mut moved = planting_input(profile, second);
        moved.moved_on = Some(crate::calendar::Date::new(2026, 10, 31).unwrap());
        store.update_planting(id, &moved).unwrap();

        let result = store.finish_planting(
            id,
            crate::calendar::Date::new(2026, 10, 30).unwrap(),
            "harvested",
        );
        assert!(result.is_err());
        assert!(
            store
                .planting(id)
                .unwrap()
                .unwrap()
                .planting
                .ended_on
                .is_none()
        );
        assert_eq!(store.planting_placements(id).unwrap().len(), 2);
    }

    #[test]
    fn a_finished_planting_keeps_its_last_day_of_readings() {
        let store = Store::open_in_memory().unwrap();
        let profile = store.create_profile("Basil", None, None).unwrap();
        let space = store.create_space(&space_input(Some("Kitchen"))).unwrap();
        let id = store
            .create_planting(&planting_input(profile, space))
            .unwrap();
        store
            .create_climate_reading(&reading_input(space, 30, Some("09:00"), Some(20.0), None))
            .unwrap();
        store
            .finish_planting(
                id,
                crate::calendar::Date::new(2026, 10, 30).unwrap(),
                "harvested",
            )
            .unwrap();

        assert_eq!(store.planting_climate_readings(id).unwrap().len(), 1);
    }

    #[test]
    fn moving_a_planting_records_a_second_placement() {
        let store = Store::open_in_memory().unwrap();
        let profile = store.create_profile("Basil", None, None).unwrap();
        let first = store.create_space(&space_input(Some("Kitchen"))).unwrap();
        let second = store
            .create_space(&space_input(Some("Greenhouse")))
            .unwrap();
        let id = store
            .create_planting(&planting_input(profile, first))
            .unwrap();

        let mut moved = planting_input(profile, second);
        moved.moved_on = Some(crate::calendar::Date::new(2026, 10, 31).unwrap());
        store.update_planting(id, &moved).unwrap();

        let placements = store.planting_placements(id).unwrap();
        assert_eq!(placements.len(), 2);
        assert_eq!(placements[0].space_id, first);
        assert_eq!(
            placements[0].ended_on,
            Some(crate::calendar::Date::new(2026, 10, 30).unwrap())
        );
        assert_eq!(placements[1].space_id, second);
        assert_eq!(placements[1].ended_on, None);
    }

    #[test]
    fn a_same_day_move_updates_the_placement() {
        let store = Store::open_in_memory().unwrap();
        let profile = store.create_profile("Basil", None, None).unwrap();
        let first = store.create_space(&space_input(Some("Kitchen"))).unwrap();
        let mut second_input = space_input(Some("Greenhouse"));
        second_input.light_start = Some(18 * 60);
        let second = store.create_space(&second_input).unwrap();
        let id = store
            .create_planting(&planting_input(profile, first))
            .unwrap();

        let mut moved = planting_input(profile, second);
        moved.moved_on = Some(crate::calendar::Date::new(2026, 10, 30).unwrap());
        store.update_planting(id, &moved).unwrap();

        let placements = store.planting_placements(id).unwrap();
        assert_eq!(placements.len(), 1);
        assert_eq!(placements[0].space_id, second);
        assert_eq!(placements[0].light_start, Some(18 * 60));
    }

    #[test]
    fn a_future_move_is_refused() {
        let store = Store::open_in_memory().unwrap();
        let profile = store.create_profile("Basil", None, None).unwrap();
        let first = store.create_space(&space_input(Some("Kitchen"))).unwrap();
        let second = store
            .create_space(&space_input(Some("Greenhouse")))
            .unwrap();
        let id = store
            .create_planting(&planting_input(profile, first))
            .unwrap();

        let mut moved = planting_input(profile, second);
        moved.moved_on = Some(crate::calendar::Date::today().add_days(2));
        assert!(store.update_planting(id, &moved).is_err());
    }

    #[test]
    fn the_start_date_can_move_within_the_first_placement() {
        let store = Store::open_in_memory().unwrap();
        let profile = store.create_profile("Basil", None, None).unwrap();
        let first = store.create_space(&space_input(Some("Kitchen"))).unwrap();
        let second = store
            .create_space(&space_input(Some("Greenhouse")))
            .unwrap();
        let id = store
            .create_planting(&planting_input(profile, first))
            .unwrap();
        let mut moved = planting_input(profile, second);
        moved.moved_on = Some(crate::calendar::Date::new(2026, 10, 31).unwrap());
        store.update_planting(id, &moved).unwrap();

        let mut edited = planting_input(profile, second);
        edited.started_on = crate::calendar::Date::new(2026, 10, 29).unwrap();
        store.update_planting(id, &edited).unwrap();

        let placements = store.planting_placements(id).unwrap();
        assert_eq!(
            placements[0].started_on,
            crate::calendar::Date::new(2026, 10, 29).unwrap()
        );
    }

    #[test]
    fn planting_readings_follow_the_placements() {
        let store = Store::open_in_memory().unwrap();
        let profile = store.create_profile("Basil", None, None).unwrap();
        let first = store.create_space(&space_input(Some("Kitchen"))).unwrap();
        let second = store
            .create_space(&space_input(Some("Greenhouse")))
            .unwrap();
        let id = store
            .create_planting(&planting_input(profile, first))
            .unwrap();
        store
            .create_climate_reading(&reading_input(first, 30, Some("09:00"), Some(20.0), None))
            .unwrap();

        let mut moved = planting_input(profile, second);
        moved.moved_on = Some(crate::calendar::Date::new(2026, 10, 31).unwrap());
        store.update_planting(id, &moved).unwrap();
        store
            .create_climate_reading(&reading_input(second, 31, Some("09:00"), Some(24.0), None))
            .unwrap();

        let readings = store.planting_climate_readings(id).unwrap();
        assert_eq!(readings.len(), 2);
        assert_eq!(readings[0].temperature_c, Some(24.0));
        assert_eq!(readings[1].temperature_c, Some(20.0));
    }

    #[test]
    fn a_space_with_history_keeps_its_type() {
        let store = Store::open_in_memory().unwrap();
        let profile = store.create_profile("Basil", None, None).unwrap();
        let space = store.create_space(&space_input(Some("Kitchen"))).unwrap();
        store
            .create_planting(&planting_input(profile, space))
            .unwrap();

        let mut changed = space_input(Some("Kitchen"));
        changed.kind = "outdoor".into();
        changed.light_type = None;
        changed.light_hours = None;
        changed.light_start = None;
        let error = store.update_space(space, &changed).unwrap_err().to_string();
        assert!(error.contains("keeps its type"));

        let mut renamed = space_input(Some("Utility room"));
        renamed.light_hours = Some(10.0);
        store.update_space(space, &renamed).unwrap();
        let space = store.space(space).unwrap().unwrap();
        assert_eq!(space.display_name(), Some("Utility room"));
        assert_eq!(space.kind, "indoor");
        assert_eq!(space.light_hours, Some(10.0));
    }

    #[test]
    fn deleting_a_planting_removes_its_private_spaces() {
        let store = Store::open_in_memory().unwrap();
        let profile = store.create_profile("Basil", None, None).unwrap();
        let private = store.create_space(&space_input(None)).unwrap();
        let id = store
            .create_planting(&planting_input(profile, private))
            .unwrap();
        store
            .create_climate_reading(&reading_input(private, 30, Some("09:00"), Some(20.0), None))
            .unwrap();

        store.delete_planting(id).unwrap();

        assert!(store.space(private).unwrap().is_none());
        assert!(store.climate_readings(private).unwrap().is_empty());
    }

    #[test]
    fn a_private_space_holds_one_planting() {
        let store = Store::open_in_memory().unwrap();
        let profile = store.create_profile("Monstera", None, None).unwrap();
        let private = store.create_space(&space_input(None)).unwrap();

        store
            .create_planting(&planting_input(profile, private))
            .unwrap();
        assert!(
            store
                .create_planting(&planting_input(profile, private))
                .is_err()
        );
    }

    #[test]
    fn cannot_move_into_an_occupied_private_space() {
        let store = Store::open_in_memory().unwrap();
        let profile = store.create_profile("Basil", None, None).unwrap();
        let one = store.create_space(&space_input(None)).unwrap();
        let two = store.create_space(&space_input(None)).unwrap();
        store
            .create_planting(&planting_input(profile, one))
            .unwrap();
        let moving = store
            .create_planting(&planting_input(profile, two))
            .unwrap();

        assert!(
            store
                .update_planting(moving, &planting_input(profile, one))
                .is_err()
        );
        assert_eq!(
            store.planting(moving).unwrap().unwrap().planting.space_id,
            two
        );
    }

    #[test]
    fn a_shared_space_cannot_lose_its_name() {
        let store = Store::open_in_memory().unwrap();
        let profile = store.create_profile("Basil", None, None).unwrap();

        let one = store.create_space(&space_input(Some("Kitchen"))).unwrap();
        store
            .create_planting(&planting_input(profile, one))
            .unwrap();
        store.update_space(one, &space_input(None)).unwrap();
        assert_eq!(store.space(one).unwrap().unwrap().name, None);

        let two = store.create_space(&space_input(Some("Grow room"))).unwrap();
        store
            .create_planting(&planting_input(profile, two))
            .unwrap();
        store
            .create_planting(&planting_input(profile, two))
            .unwrap();
        assert!(store.update_space(two, &space_input(None)).is_err());
        assert_eq!(
            store.space(two).unwrap().unwrap().name.as_deref(),
            Some("Grow room")
        );
    }

    #[test]
    fn a_planting_cannot_change_its_profile() {
        let store = Store::open_in_memory().unwrap();
        let basil = store.create_profile("Basil", None, None).unwrap();
        let mint = store.create_profile("Mint", None, None).unwrap();
        let space = store.create_space(&space_input(Some("Kitchen"))).unwrap();
        let id = store
            .create_planting(&planting_input(basil, space))
            .unwrap();

        assert!(
            store
                .update_planting(id, &planting_input(mint, space))
                .is_err()
        );
        assert_eq!(
            store.planting(id).unwrap().unwrap().planting.profile_id,
            basil
        );
    }

    #[test]
    fn keeps_a_named_space_when_its_last_planting_goes() {
        let store = Store::open_in_memory().unwrap();
        let profile = store.create_profile("Basil", None, None).unwrap();
        let space = store.create_space(&space_input(Some("Kitchen"))).unwrap();
        let id = store
            .create_planting(&planting_input(profile, space))
            .unwrap();

        store.delete_planting(id).unwrap();

        assert!(store.space(space).unwrap().is_some());
        assert!(store.plantings().unwrap().is_empty());
    }

    #[test]
    fn finishes_and_reopens_a_planting() {
        let store = Store::open_in_memory().unwrap();
        let profile = store.create_profile("Tomato", None, None).unwrap();
        let space = store
            .create_space(&space_input(Some("Greenhouse")))
            .unwrap();
        let id = store
            .create_planting(&planting_input(profile, space))
            .unwrap();
        let today = crate::calendar::Date::new(2026, 10, 30).unwrap();

        store.finish_planting(id, today, "harvested").unwrap();
        let finished = store.planting(id).unwrap().unwrap().planting;
        assert_eq!(
            finished.state(today),
            crate::planting::PlantingState::Finished
        );
        assert_eq!(finished.outcome.as_deref(), Some("harvested"));

        store.reopen_planting(id).unwrap();
        let reopened = store.planting(id).unwrap().unwrap().planting;
        assert_eq!(reopened.ended_on, None);
        assert_eq!(reopened.outcome, None);
        assert_eq!(
            reopened.state(today),
            crate::planting::PlantingState::Growing
        );

        let before = crate::calendar::Date::new(2026, 9, 1).unwrap();
        assert!(store.finish_planting(id, before, "harvested").is_err());
        assert!(store.finish_planting(id, today, "eaten").is_err());
        assert!(store.reopen_planting(999).is_err());
    }

    #[test]
    fn refuses_duplicate_space_names() {
        let store = Store::open_in_memory().unwrap();
        let space = store.create_space(&space_input(Some("Grow tent"))).unwrap();
        assert!(store.create_space(&space_input(Some("grow tent"))).is_err());

        let other = store
            .create_space(&space_input(Some("Greenhouse")))
            .unwrap();
        let mut rename = space_input(Some("GROW TENT"));
        rename.kind = "outdoor".into();
        rename.light_type = None;
        rename.light_hours = None;
        assert!(store.update_space(other, &rename).is_err());

        rename.name = Some("Grow tent".into());
        // "Grow tent" is still taken by `space`.
        assert!(store.update_space(other, &rename).is_err());
        assert_eq!(
            store.space(space).unwrap().unwrap().name.as_deref(),
            Some("Grow tent")
        );
    }

    #[test]
    fn an_outdoor_space_has_no_light_settings() {
        let store = Store::open_in_memory().unwrap();
        let mut input = space_input(Some("Garden"));
        input.kind = "outdoor".into();
        assert!(store.create_space(&input).is_err());

        input.light_type = None;
        input.light_hours = None;
        input.light_start = None;
        assert!(store.create_space(&input).is_ok());
    }

    fn observation_input(planting_id: i64, kind: &str) -> ObservationInput {
        ObservationInput {
            planting_id,
            kind: kind.into(),
            occurred_on: crate::calendar::Date::new(2026, 10, 30).unwrap(),
            occurred_time: None,
            note: None,
            amount: None,
            unit: None,
            stage_id: None,
        }
    }

    #[test]
    fn stores_and_deletes_a_diary_entry() {
        let store = Store::open_in_memory().unwrap();
        let profile = store.create_profile("Tomato", None, None).unwrap();
        let space = store
            .create_space(&space_input(Some("Greenhouse")))
            .unwrap();
        let planting = store
            .create_planting(&planting_input(profile, space))
            .unwrap();

        let mut input = observation_input(planting, "water");
        input.amount = Some(2.0);
        input.unit = Some("L".into());
        input.occurred_time = Some("9:05".into());
        input.note = Some("  Deep watering.  ".into());
        let id = store.create_observation(&input).unwrap();

        let observation = store.observation(id).unwrap().unwrap();
        assert_eq!(observation.occurred_time.as_deref(), Some("09:05"));
        assert_eq!(observation.note.as_deref(), Some("Deep watering."));
        assert_eq!(observation.amount, Some(2.0));
        assert_eq!(store.observations(planting).unwrap().len(), 1);

        store.delete_observation(id).unwrap();
        assert!(store.observations(planting).unwrap().is_empty());

        let mut early = observation_input(planting, "note");
        early.occurred_on = crate::calendar::Date::new(2026, 9, 1).unwrap();
        assert!(store.create_observation(&early).is_err());
    }

    #[test]
    fn a_repot_keeps_the_old_volume() {
        let store = Store::open_in_memory().unwrap();
        let profile = store.create_profile("Monstera", None, None).unwrap();
        let space = store
            .create_space(&space_input(Some("Living room")))
            .unwrap();
        let planting = store
            .create_planting(&planting_input(profile, space))
            .unwrap();

        let mut input = observation_input(planting, "repot");
        input.amount = Some(11.0);
        input.unit = Some("L".into());
        let id = store.create_observation(&input).unwrap();

        let observation = store.observation(id).unwrap().unwrap();
        assert_eq!(observation.previous_amount, Some(3.0));
        assert_eq!(observation.previous_unit.as_deref(), Some("L"));
        assert_eq!(observation.amount, Some(11.0));
        assert_eq!(
            store
                .planting(planting)
                .unwrap()
                .unwrap()
                .planting
                .container_volume_l,
            Some(11.0)
        );

        // A repot only belongs to a pot, not to open ground.
        let ground_profile = store.create_profile("Maize", None, None).unwrap();
        let ground_space = store.create_space(&space_input(Some("Field"))).unwrap();
        let ground = store
            .create_planting(&PlantingInput {
                container: "ground".into(),
                container_volume_l: None,
                ..planting_input(ground_profile, ground_space)
            })
            .unwrap();
        let mut repot = observation_input(ground, "repot");
        repot.amount = Some(5.0);
        repot.unit = Some("L".into());
        assert!(store.create_observation(&repot).is_err());
    }

    #[test]
    fn a_stage_entry_belongs_to_the_profiles_stages() {
        let store = Store::open_in_memory().unwrap();
        let profile = store.create_profile("Tomato", None, None).unwrap();
        let other = store.create_profile("Basil", None, None).unwrap();
        let space = store
            .create_space(&space_input(Some("Greenhouse")))
            .unwrap();
        let planting = store
            .create_planting(&planting_input(profile, space))
            .unwrap();
        let stage = store
            .create_stage(&new_stage(profile, 1, "flowering"))
            .unwrap();
        let foreign = store
            .create_stage(&new_stage(other, 1, "flowering"))
            .unwrap();

        let mut input = observation_input(planting, "stage");
        input.stage_id = Some(stage);
        store.create_observation(&input).unwrap();
        assert_eq!(
            store.stage_starts(planting).unwrap(),
            [(stage, crate::calendar::Date::new(2026, 10, 30).unwrap())]
        );

        input.stage_id = Some(foreign);
        assert!(store.create_observation(&input).is_err());
    }

    #[test]
    fn stage_starts_keep_every_cycle_oldest_first() {
        let store = Store::open_in_memory().unwrap();
        let profile = store.create_profile("Tomato", None, None).unwrap();
        let space = store
            .create_space(&space_input(Some("Greenhouse")))
            .unwrap();
        let planting = store
            .create_planting(&planting_input(profile, space))
            .unwrap();
        let stage = store
            .create_stage(&new_stage(profile, 1, "flowering"))
            .unwrap();

        let mut first = observation_input(planting, "stage");
        first.stage_id = Some(stage);
        let mut later = observation_input(planting, "stage");
        later.stage_id = Some(stage);
        later.occurred_on = crate::calendar::Date::new(2026, 10, 31).unwrap();

        store.create_observation(&later).unwrap();
        store.create_observation(&first).unwrap();

        // Two cycles of the same stage both keep their date; the schedule matches each to its own
        // cycle.
        assert_eq!(
            store.stage_starts(planting).unwrap(),
            [
                (stage, crate::calendar::Date::new(2026, 10, 30).unwrap()),
                (stage, crate::calendar::Date::new(2026, 10, 31).unwrap()),
            ]
        );
    }

    #[test]
    fn a_deleted_stage_leaves_the_entry_without_a_stage() {
        let store = Store::open_in_memory().unwrap();
        let profile = store.create_profile("Tomato", None, None).unwrap();
        let space = store
            .create_space(&space_input(Some("Greenhouse")))
            .unwrap();
        let planting = store
            .create_planting(&planting_input(profile, space))
            .unwrap();
        let stage = store
            .create_stage(&new_stage(profile, 1, "flowering"))
            .unwrap();

        let mut input = observation_input(planting, "stage");
        input.stage_id = Some(stage);
        let observation = store.create_observation(&input).unwrap();

        store.delete_stage(stage).unwrap();

        assert_eq!(
            store.observation(observation).unwrap().unwrap().stage_id,
            None
        );
        assert!(store.stage_starts(planting).unwrap().is_empty());
    }

    #[test]
    fn a_repot_without_a_volume_keeps_the_size() {
        let store = Store::open_in_memory().unwrap();
        let profile = store.create_profile("Monstera", None, None).unwrap();
        let space = store
            .create_space(&space_input(Some("Living room")))
            .unwrap();
        let planting = store
            .create_planting(&planting_input(profile, space))
            .unwrap();

        let observation = store
            .create_observation(&observation_input(planting, "repot"))
            .unwrap();
        let observation = store.observation(observation).unwrap().unwrap();

        assert_eq!(observation.previous_amount, Some(3.0));
        assert_eq!(observation.amount, None);
        assert_eq!(
            store
                .planting(planting)
                .unwrap()
                .unwrap()
                .planting
                .container_volume_l,
            Some(3.0)
        );
    }

    #[test]
    fn discarding_a_repot_restores_the_volume() {
        let store = Store::open_in_memory().unwrap();
        let profile = store.create_profile("Monstera", None, None).unwrap();
        let space = store
            .create_space(&space_input(Some("Living room")))
            .unwrap();
        let planting = store
            .create_planting(&planting_input(profile, space))
            .unwrap();

        let mut input = observation_input(planting, "repot");
        input.amount = Some(11.0);
        input.unit = Some("L".into());
        let observation = store.create_observation(&input).unwrap();
        assert_eq!(
            store
                .planting(planting)
                .unwrap()
                .unwrap()
                .planting
                .container_volume_l,
            Some(11.0)
        );

        store.discard_observation(observation).unwrap();

        assert!(store.observations(planting).unwrap().is_empty());
        assert_eq!(
            store
                .planting(planting)
                .unwrap()
                .unwrap()
                .planting
                .container_volume_l,
            Some(3.0)
        );
    }

    #[test]
    fn deleting_a_planting_cleans_up_diary_media() {
        let store = Store::open_in_memory().unwrap();
        let profile = store.create_profile("Tomato", None, None).unwrap();
        let space = store
            .create_space(&space_input(Some("Greenhouse")))
            .unwrap();
        let planting = store
            .create_planting(&planting_input(profile, space))
            .unwrap();
        let observation = store
            .create_observation(&observation_input(planting, "note"))
            .unwrap();
        let stem = format!("observation-{observation}-photo");
        let avatar = format!("planting-{planting}-avatar");
        for (owner_type, owner_id, role, file_stem) in [
            ("observation", observation, "photo", stem.clone()),
            ("planting", planting, "avatar", avatar.clone()),
        ] {
            store
                .set_media(&Media {
                    id: 0,
                    owner_type: owner_type.into(),
                    owner_id,
                    role: role.into(),
                    file_stem,
                    width: 100,
                    height: 100,
                    bytes: 10,
                })
                .unwrap();
        }

        let stems = store.delete_planting(planting).unwrap();
        assert!(stems.contains(&stem));
        assert!(stems.contains(&avatar));

        assert!(store.observations(planting).unwrap().is_empty());
        assert!(
            store
                .media_for("observation", observation)
                .unwrap()
                .is_empty()
        );
        assert!(store.media_for("planting", planting).unwrap().is_empty());
        assert_eq!(
            store.delete_observation(observation).unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn deleting_a_planting_returns_its_private_space_avatar() {
        let store = Store::open_in_memory().unwrap();
        let profile = store.create_profile("Basil", None, None).unwrap();
        let private = store.create_space(&space_input(None)).unwrap();
        let planting = store
            .create_planting(&planting_input(profile, private))
            .unwrap();
        let stem = format!("space-{private}-avatar");
        store
            .set_media(&Media {
                id: 0,
                owner_type: "space".into(),
                owner_id: private,
                role: "avatar".into(),
                file_stem: stem.clone(),
                width: 100,
                height: 100,
                bytes: 10,
            })
            .unwrap();

        let stems = store.delete_planting(planting).unwrap();

        assert!(stems.contains(&stem));
        assert!(store.space(private).unwrap().is_none());
        assert!(store.media_for("space", private).unwrap().is_empty());
    }

    #[test]
    fn deleting_an_entry_returns_its_media_for_cleanup() {
        let store = Store::open_in_memory().unwrap();
        let profile = store.create_profile("Tomato", None, None).unwrap();
        let space = store
            .create_space(&space_input(Some("Greenhouse")))
            .unwrap();
        let planting = store
            .create_planting(&planting_input(profile, space))
            .unwrap();
        let observation = store
            .create_observation(&observation_input(planting, "note"))
            .unwrap();
        let stem = format!("observation-{observation}-photo");
        store
            .set_media(&Media {
                id: 0,
                owner_type: "observation".into(),
                owner_id: observation,
                role: "photo".into(),
                file_stem: stem.clone(),
                width: 100,
                height: 100,
                bytes: 10,
            })
            .unwrap();

        assert_eq!(store.delete_observation(observation).unwrap(), [stem]);
    }

    #[test]
    fn repeating_a_planting_copies_the_setup() {
        let store = Store::open_in_memory().unwrap();
        let profile = store.create_profile("Tomato", None, None).unwrap();
        let space = store
            .create_space(&space_input(Some("Greenhouse")))
            .unwrap();
        let source = store
            .create_planting(&planting_input(profile, space))
            .unwrap();
        store
            .create_observation(&observation_input(source, "note"))
            .unwrap();

        let repeat = store
            .repeat_planting(source, crate::calendar::Date::new(2027, 4, 1).unwrap())
            .unwrap();
        let repeat_summary = store.planting(repeat).unwrap().unwrap();
        let source_summary = store.planting(source).unwrap().unwrap();

        assert_eq!(
            repeat_summary.planting.space_id,
            source_summary.planting.space_id
        );
        assert_eq!(repeat_summary.planting.profile_id, profile);
        assert_eq!(
            repeat_summary.planting.quantity,
            source_summary.planting.quantity
        );
        assert_eq!(
            repeat_summary.planting.container_volume_l,
            source_summary.planting.container_volume_l
        );
        assert_eq!(
            repeat_summary.planting.started_on,
            crate::calendar::Date::new(2027, 4, 1).unwrap()
        );
        assert!(store.observations(repeat).unwrap().is_empty());
    }

    #[test]
    fn repeating_a_private_planting_gets_a_new_private_space() {
        let store = Store::open_in_memory().unwrap();
        let profile = store.create_profile("Monstera", None, None).unwrap();
        let private = store.create_space(&space_input(None)).unwrap();
        let source = store
            .create_planting(&planting_input(profile, private))
            .unwrap();

        let repeat = store
            .repeat_planting(source, crate::calendar::Date::new(2027, 4, 1).unwrap())
            .unwrap();
        let repeat_summary = store.planting(repeat).unwrap().unwrap();

        assert_ne!(repeat_summary.planting.space_id, private);
        assert_eq!(
            store
                .space(repeat_summary.planting.space_id)
                .unwrap()
                .unwrap()
                .name,
            None
        );
        assert_eq!(store.spaces().unwrap().len(), 0);
    }

    #[test]
    fn stores_climate_readings_newest_first() {
        let store = Store::open_in_memory().unwrap();
        let space = store.create_space(&space_input(Some("Grow tent"))).unwrap();

        let first = store
            .create_climate_reading(&reading_input(space, 29, Some("09:00"), Some(18.5), None))
            .unwrap();
        let second = store
            .create_climate_reading(&reading_input(space, 30, None, Some(22.0), Some(61.0)))
            .unwrap();

        let readings = store.climate_readings(space).unwrap();
        assert_eq!(readings.len(), 2);
        assert_eq!(readings[0].id, second);
        assert_eq!(readings[1].id, first);
        assert_eq!(readings[0].humidity_pct, Some(61.0));
        assert_eq!(readings[1].temperature_c, Some(18.5));
        assert_eq!(readings[1].measured_time.as_deref(), Some("09:00"));

        store.delete_climate_reading(second).unwrap();
        assert_eq!(store.climate_readings(space).unwrap().len(), 1);
    }

    #[test]
    fn a_climate_reading_needs_a_value_and_a_real_space() {
        let store = Store::open_in_memory().unwrap();
        let space = store.create_space(&space_input(Some("Grow tent"))).unwrap();

        let empty = reading_input(space, 30, None, None, None);
        assert!(store.create_climate_reading(&empty).is_err());

        let missing = reading_input(999, 30, None, Some(21.0), None);
        assert!(store.create_climate_reading(&missing).is_err());
    }

    fn weather_current() -> WeatherCurrent {
        WeatherCurrent {
            latitude: 51.2089,
            longitude: 3.2242,
            observed_on: crate::calendar::Date::new(2026, 10, 31).unwrap(),
            observed_time: Some("09:15".to_string()),
            temperature_c: Some(12.4),
            humidity_pct: Some(87.0),
            apparent_temperature_c: Some(11.0),
            weather_code: Some(2),
            precipitation_mm: Some(0.2),
            cloud_cover_pct: Some(60.0),
            wind_speed_kmh: Some(12.0),
            wind_gust_kmh: Some(24.0),
            is_day: Some(true),
        }
    }

    #[test]
    fn weather_current_is_deduped_and_read_back() {
        let store = Store::open_in_memory().unwrap();
        let current = weather_current();

        assert!(store.insert_weather_current(&current).unwrap());
        assert!(!store.insert_weather_current(&current).unwrap());

        let latest = store
            .weather_current_latest(51.2089, 3.2242)
            .unwrap()
            .unwrap();
        assert_eq!(latest.temperature_c, Some(12.4));
        assert_eq!(latest.weather_code, Some(2));
        assert!(store.weather_current_latest(51.0, 3.0).unwrap().is_none());
    }

    #[test]
    fn weather_days_upsert_and_filter_by_location() {
        let store = Store::open_in_memory().unwrap();
        let date = crate::calendar::Date::new(2026, 10, 31).unwrap();
        let mut day = WeatherDay {
            latitude: 51.2089,
            longitude: 3.2242,
            date,
            temp_min_c: Some(8.0),
            temp_max_c: Some(14.0),
            temp_mean_c: Some(11.0),
            humidity_mean_pct: Some(80.0),
            humidity_min_pct: Some(65.0),
            humidity_max_pct: Some(90.0),
            precipitation_mm: Some(1.2),
            precipitation_hours: Some(2.0),
            weather_code: Some(3),
            sunshine_hours: Some(2.0),
            daylight_hours: Some(10.0),
            radiation_sum_mj_m2: Some(6.5),
            wind_max_kmh: Some(18.0),
            wind_gust_max_kmh: Some(30.0),
            et0_mm: Some(1.2),
            sunrise: Some("07:35".to_string()),
            sunset: Some("17:20".to_string()),
            night_start: Some("2026-10-30T22:00".to_string()),
            night_end: Some("2026-10-31T06:00".to_string()),
            night_temp_avg_c: Some(9.0),
            night_temp_min_c: Some(8.0),
            night_humidity_avg_pct: Some(82.0),
            night_hours: Some(3.0),
            night_complete: true,
        };

        store.upsert_weather_day(&day).unwrap();
        day.precipitation_mm = Some(2.4);
        store.upsert_weather_day(&day).unwrap();

        // An answer without hourly data must not wipe a good night.
        let mut incomplete = day.clone();
        incomplete.night_start = None;
        incomplete.night_end = None;
        incomplete.night_temp_avg_c = None;
        incomplete.night_temp_min_c = None;
        incomplete.night_humidity_avg_pct = None;
        incomplete.night_hours = None;
        incomplete.night_complete = false;
        store.upsert_weather_day(&incomplete).unwrap();

        let days = store.weather_days(51.2089, 3.2242, date).unwrap();
        assert_eq!(days.len(), 1);
        assert_eq!(days[0].precipitation_mm, Some(2.4));
        assert_eq!(days[0].night_temp_avg_c, Some(9.0));

        // A partial day answer keeps the values that are already stored.
        let mut partial = day.clone();
        partial.temp_min_c = None;
        partial.temp_max_c = Some(15.0);
        partial.humidity_mean_pct = None;
        store.upsert_weather_day(&partial).unwrap();
        let days = store.weather_days(51.2089, 3.2242, date).unwrap();
        assert_eq!(days[0].temp_min_c, Some(8.0));
        assert_eq!(days[0].temp_max_c, Some(15.0));
        assert_eq!(days[0].humidity_mean_pct, Some(80.0));

        assert!(
            store
                .weather_days(51.2089, 3.2242, date.add_days(1))
                .unwrap()
                .is_empty()
        );
        assert!(store.weather_days(51.0, 3.0, date).unwrap().is_empty());
    }

    #[test]
    fn schema_sixteen_has_the_hourly_table_and_the_light_start() {
        let store = Store::open_in_memory().unwrap();
        let has_column = |table: &str, column: &str| -> bool {
            store
                .conn
                .query_row(
                    "SELECT EXISTS(
                         SELECT 1 FROM pragma_table_info(?1) WHERE name = ?2
                     )",
                    [table, column],
                    |row| row.get(0),
                )
                .unwrap()
        };
        assert!(has_column("weather_daily", "sunrise"));
        assert!(has_column("weather_daily", "sunset"));
        assert!(has_column("spaces", "light_start"));
        assert!(has_column("planting_placements", "light_start"));
        assert!(has_column("weather_hours", "is_day"));
    }

    #[test]
    fn weather_hours_upsert_and_filter_by_location_and_date() {
        let store = Store::open_in_memory().unwrap();
        let date = crate::calendar::Date::new(2026, 10, 31).unwrap();
        let mut hour = WeatherHour {
            latitude: 51.2089,
            longitude: 3.2242,
            date,
            time: "14:00".into(),
            minutes: 14 * 60,
            temperature_c: Some(14.2),
            humidity_pct: Some(68.0),
            precipitation_mm: Some(0.0),
            weather_code: Some(2),
            cloud_cover_pct: Some(40.0),
            sunshine_hours: Some(1.0),
            is_day: Some(true),
        };
        store.upsert_weather_hour(&hour).unwrap();

        hour.temperature_c = Some(15.0);
        store.upsert_weather_hour(&hour).unwrap();

        let hours = store.weather_hours(51.2089, 3.2242, date, date).unwrap();
        assert_eq!(hours.len(), 1);
        assert_eq!(hours[0].temperature_c, Some(15.0));
        assert_eq!(hours[0].is_day, Some(true));
        assert_eq!(hours[0].minutes, 14 * 60);

        assert!(
            store
                .weather_hours(51.0, 3.0, date, date)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_space_keeps_the_light_start_and_a_placement_snapshots_it() {
        let store = Store::open_in_memory().unwrap();
        let profile = store.create_profile("Basil", None, None).unwrap();
        let mut input = space_input(Some("Tent"));
        input.light_start = Some(6 * 60);
        let space = store.create_space(&input).unwrap();
        assert_eq!(
            store.space(space).unwrap().unwrap().light_start,
            Some(6 * 60)
        );

        let planting = store
            .create_planting(&planting_input(profile, space))
            .unwrap();
        let placements = store.planting_placements(planting).unwrap();
        assert_eq!(placements[0].light_start, Some(6 * 60));

        input.light_start = Some(18 * 60);
        store.update_space(space, &input).unwrap();
        let placements = store.planting_placements(planting).unwrap();
        assert_eq!(placements[0].light_start, Some(18 * 60));
    }

    fn weather_day(date: crate::calendar::Date) -> WeatherDay {
        WeatherDay {
            latitude: 51.2089,
            longitude: 3.2242,
            date,
            temp_min_c: Some(8.0),
            temp_max_c: Some(16.0),
            temp_mean_c: Some(12.0),
            humidity_mean_pct: Some(80.0),
            humidity_min_pct: Some(60.0),
            humidity_max_pct: Some(95.0),
            precipitation_mm: Some(0.0),
            precipitation_hours: Some(0.0),
            weather_code: Some(2),
            sunshine_hours: Some(4.0),
            daylight_hours: Some(10.0),
            radiation_sum_mj_m2: Some(8.0),
            wind_max_kmh: Some(12.0),
            wind_gust_max_kmh: Some(20.0),
            et0_mm: Some(1.0),
            sunrise: Some("07:35".to_string()),
            sunset: Some("17:20".to_string()),
            night_start: None,
            night_end: None,
            night_temp_avg_c: None,
            night_temp_min_c: None,
            night_humidity_avg_pct: None,
            night_hours: None,
            night_complete: false,
        }
    }

    #[test]
    fn planting_weather_follows_outdoor_placements() {
        let store = Store::open_in_memory().unwrap();
        store
            .set_location(Location {
                latitude: 51.2089,
                longitude: 3.2242,
                name: Some("Bruges".to_string()),
                region: None,
                country: None,
                timezone: None,
            })
            .unwrap();
        let profile = store.create_profile("Basil", None, None).unwrap();
        let mut outdoor = space_input(Some("Garden"));
        outdoor.kind = "outdoor".into();
        outdoor.light_type = None;
        outdoor.light_hours = None;
        outdoor.light_start = None;
        let outdoor = store.create_space(&outdoor).unwrap();
        let indoor = store.create_space(&space_input(Some("Kitchen"))).unwrap();
        let id = store
            .create_planting(&planting_input(profile, outdoor))
            .unwrap();

        let first = crate::calendar::Date::new(2026, 10, 30).unwrap();
        let second = crate::calendar::Date::new(2026, 10, 31).unwrap();
        store.upsert_weather_day(&weather_day(first)).unwrap();
        store.upsert_weather_day(&weather_day(second)).unwrap();

        assert_eq!(store.planting_weather_days(id).unwrap().len(), 2);

        let mut moved = planting_input(profile, indoor);
        moved.moved_on = Some(second);
        store.update_planting(id, &moved).unwrap();

        let weather = store.planting_weather_days(id).unwrap();
        assert_eq!(weather.len(), 1);
        assert_eq!(weather[0].date, first);
    }

    #[test]
    fn migrates_weather_rows_without_a_location() {
        let path = std::env::temp_dir().join(format!(
            "omagrow-weather-nolocation-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_file(&path);

        {
            let store = Store::open(&path).unwrap();
            let space = store.create_space(&space_input(Some("Garden"))).unwrap();
            store
                .conn
                .execute_batch(
                    "DROP TABLE weather_current;
                     DROP TABLE weather_daily;
                     DROP TABLE planting_placements;
                     PRAGMA user_version = 12;",
                )
                .unwrap();
            store
                .conn
                .execute(
                    "INSERT INTO climate_readings
                         (space_id, measured_on, measured_time, temperature_c, humidity_pct,
                          source, source_ref)
                     VALUES (?1, '2026-10-31', '09:00', 12.4, 87, 'weather', 'Open-Meteo')",
                    [space],
                )
                .unwrap();
        }

        let store = Store::open(&path).unwrap();
        let current = store.weather_current_latest(0.0, 0.0).unwrap().unwrap();
        assert_eq!(current.temperature_c, Some(12.4));
        assert!(store.climate_readings(1).unwrap().is_empty());

        drop(store);
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(path.with_extension("db-wal"));
        let _ = fs::remove_file(path.with_extension("db-shm"));
    }

    #[test]
    fn migrates_plantings_into_placements() {
        let path = std::env::temp_dir().join(format!(
            "omagrow-placement-migration-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_file(&path);

        let planting = {
            let store = Store::open(&path).unwrap();
            let profile = store.create_profile("Basil", None, None).unwrap();
            let space = store.create_space(&space_input(Some("Kitchen"))).unwrap();
            let planting = store
                .create_planting(&planting_input(profile, space))
                .unwrap();
            store
                .conn
                .execute_batch("DROP TABLE planting_placements; PRAGMA user_version = 13;")
                .unwrap();
            planting
        };

        let store = Store::open(&path).unwrap();
        let placements = store.planting_placements(planting).unwrap();
        assert_eq!(placements.len(), 1);
        assert_eq!(placements[0].space_kind, "indoor");

        drop(store);
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(path.with_extension("db-wal"));
        let _ = fs::remove_file(path.with_extension("db-shm"));
    }

    #[test]
    fn migrates_weather_rows_out_of_climate_readings() {
        let path = std::env::temp_dir().join(format!(
            "omagrow-weather-migration-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_file(&path);

        {
            let store = Store::open(&path).unwrap();
            store
                .set_location(Location {
                    latitude: 51.2089,
                    longitude: 3.2242,
                    name: Some("Bruges".to_string()),
                    region: None,
                    country: Some("Belgium".to_string()),
                    timezone: Some("Europe/Brussels".to_string()),
                })
                .unwrap();
            let space = store.create_space(&space_input(Some("Garden"))).unwrap();
            store
                .conn
                .execute_batch(
                    "DROP TABLE weather_current;
                     DROP TABLE weather_daily;
                     DROP TABLE planting_placements;
                     PRAGMA user_version = 12;",
                )
                .unwrap();
            store
                .conn
                .execute(
                    "INSERT INTO climate_readings
                         (space_id, measured_on, measured_time, temperature_c, humidity_pct,
                          source, source_ref)
                     VALUES (?1, '2026-10-31', '09:00', 12.4, 87, 'weather', 'Open-Meteo')",
                    [space],
                )
                .unwrap();
        }

        let store = Store::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), Store::SCHEMA_VERSION);
        assert!(store.climate_readings(1).unwrap().is_empty());
        let current = store
            .weather_current_latest(51.2089, 3.2242)
            .unwrap()
            .unwrap();
        assert_eq!(current.temperature_c, Some(12.4));

        drop(store);
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(path.with_extension("db-wal"));
        let _ = fs::remove_file(path.with_extension("db-shm"));
    }

    #[test]
    fn refuses_an_unbounded_timing_offset() {
        let store = Store::open_in_memory().unwrap();
        let id = store.create_profile("Chilli", None, None).unwrap();
        let mut rule = new_timing_rule(id);
        rule.offset_min_days = Some(i64::MAX);
        rule.offset_max_days = Some(i64::MAX);
        assert!(store.create_timing_rule(&rule).is_err());
    }

    #[test]
    fn refuses_a_reading_in_the_future() {
        let store = Store::open_in_memory().unwrap();
        let space = store
            .create_space(&space_input(Some("Greenhouse")))
            .unwrap();
        let mut input = reading_input(space, 1, None, Some(20.0), None);
        input.measured_on = crate::clock::today().add_days(1);
        assert!(store.create_climate_reading(&input).is_err());
    }

    fn reading_input(
        space_id: i64,
        day: u32,
        time: Option<&str>,
        temperature: Option<f64>,
        humidity: Option<f64>,
    ) -> ClimateInput {
        ClimateInput {
            space_id,
            measured_on: crate::calendar::Date::new(2026, 10, day).unwrap(),
            measured_time: time.map(str::to_string),
            temperature_c: temperature,
            humidity_pct: humidity,
            source: "manual".to_string(),
            source_ref: None,
        }
    }

    #[test]
    fn a_backdated_repot_keeps_the_current_volume() {
        let store = Store::open_in_memory().unwrap();
        let profile = store.create_profile("Monstera", None, None).unwrap();
        let space = store
            .create_space(&space_input(Some("Living room")))
            .unwrap();
        let planting = store
            .create_planting(&planting_input(profile, space))
            .unwrap();

        let mut newest = observation_input(planting, "repot");
        newest.occurred_on = crate::calendar::Date::new(2026, 11, 1).unwrap();
        newest.amount = Some(11.0);
        newest.unit = Some("L".into());
        let newest = store.create_observation(&newest).unwrap();

        let mut earlier = observation_input(planting, "repot");
        earlier.occurred_on = crate::calendar::Date::new(2026, 10, 31).unwrap();
        earlier.amount = Some(5.0);
        earlier.unit = Some("L".into());
        let earlier = store.create_observation(&earlier).unwrap();

        let mut earliest = observation_input(planting, "repot");
        earliest.occurred_on = crate::calendar::Date::new(2026, 10, 30).unwrap();
        earliest.amount = Some(2.0);
        earliest.unit = Some("L".into());
        let earliest = store.create_observation(&earliest).unwrap();

        assert_eq!(
            store
                .observation(earliest)
                .unwrap()
                .unwrap()
                .previous_amount,
            Some(3.0)
        );
        assert_eq!(
            store.observation(earlier).unwrap().unwrap().previous_amount,
            Some(2.0)
        );
        assert_eq!(
            store.observation(newest).unwrap().unwrap().previous_amount,
            Some(5.0)
        );
        assert_eq!(
            store
                .planting(planting)
                .unwrap()
                .unwrap()
                .planting
                .container_volume_l,
            Some(11.0)
        );

        // Removing a backdated entry restores the "before" of the entries it had rewritten.
        store.discard_observation(earliest).unwrap();
        assert_eq!(
            store.observation(earlier).unwrap().unwrap().previous_amount,
            Some(3.0)
        );
        assert_eq!(
            store.observation(newest).unwrap().unwrap().previous_amount,
            Some(5.0)
        );

        store.delete_observation(earlier).unwrap();
        assert_eq!(
            store.observation(newest).unwrap().unwrap().previous_amount,
            Some(3.0)
        );
        assert_eq!(
            store
                .planting(planting)
                .unwrap()
                .unwrap()
                .planting
                .container_volume_l,
            Some(11.0)
        );
    }

    #[test]
    fn a_repot_chain_with_an_unknown_start_does_not_guess() {
        let store = Store::open_in_memory().unwrap();
        let profile = store.create_profile("Monstera", None, None).unwrap();
        let space = store
            .create_space(&space_input(Some("Living room")))
            .unwrap();
        let mut input = planting_input(profile, space);
        input.container_volume_l = None;
        let planting = store.create_planting(&input).unwrap();

        let mut newest = observation_input(planting, "repot");
        newest.occurred_on = crate::calendar::Date::new(2026, 11, 1).unwrap();
        newest.amount = Some(5.0);
        newest.unit = Some("L".into());
        let newest = store.create_observation(&newest).unwrap();

        let mut earlier = observation_input(planting, "repot");
        earlier.occurred_on = crate::calendar::Date::new(2026, 10, 31).unwrap();
        earlier.amount = Some(2.0);
        earlier.unit = Some("L".into());
        let earlier = store.create_observation(&earlier).unwrap();

        // There is no known volume before the backdated entry; the current 5 L is not a guess.
        assert_eq!(
            store.observation(earlier).unwrap().unwrap().previous_amount,
            None
        );
        assert_eq!(
            store.observation(newest).unwrap().unwrap().previous_amount,
            Some(2.0)
        );
        assert_eq!(
            store
                .planting(planting)
                .unwrap()
                .unwrap()
                .planting
                .container_volume_l,
            Some(5.0)
        );
    }

    #[test]
    fn an_amountless_repot_does_not_block_a_backdated_size() {
        let store = Store::open_in_memory().unwrap();
        let profile = store.create_profile("Monstera", None, None).unwrap();
        let space = store
            .create_space(&space_input(Some("Living room")))
            .unwrap();
        let planting = store
            .create_planting(&planting_input(profile, space))
            .unwrap();

        let mut amountless = observation_input(planting, "repot");
        amountless.occurred_on = crate::calendar::Date::new(2026, 11, 1).unwrap();
        store.create_observation(&amountless).unwrap();

        let mut earlier = observation_input(planting, "repot");
        earlier.occurred_on = crate::calendar::Date::new(2026, 10, 31).unwrap();
        earlier.amount = Some(6.0);
        earlier.unit = Some("L".into());
        let earlier = store.create_observation(&earlier).unwrap();

        assert_eq!(
            store.observation(earlier).unwrap().unwrap().previous_amount,
            Some(3.0)
        );
        assert_eq!(
            store
                .planting(planting)
                .unwrap()
                .unwrap()
                .planting
                .container_volume_l,
            Some(6.0)
        );

        // Discarding the entry that set the volume restores it, even when the newest entry is
        // amountless.
        store.discard_observation(earlier).unwrap();
        assert_eq!(
            store
                .planting(planting)
                .unwrap()
                .unwrap()
                .planting
                .container_volume_l,
            Some(3.0)
        );
    }

    #[test]
    fn an_amountless_repot_keeps_a_manual_volume() {
        let store = Store::open_in_memory().unwrap();
        let profile = store.create_profile("Monstera", None, None).unwrap();
        let space = store
            .create_space(&space_input(Some("Living room")))
            .unwrap();
        let planting = store
            .create_planting(&planting_input(profile, space))
            .unwrap();

        let mut repot = observation_input(planting, "repot");
        repot.amount = Some(5.0);
        repot.unit = Some("L".into());
        store.create_observation(&repot).unwrap();

        let mut edited = planting_input(profile, space);
        edited.container_volume_l = Some(20.0);
        store.update_planting(planting, &edited).unwrap();

        store
            .create_observation(&observation_input(planting, "repot"))
            .unwrap();

        assert_eq!(
            store
                .planting(planting)
                .unwrap()
                .unwrap()
                .planting
                .container_volume_l,
            Some(20.0)
        );
    }

    #[test]
    fn same_day_repots_follow_the_time() {
        let store = Store::open_in_memory().unwrap();
        let profile = store.create_profile("Monstera", None, None).unwrap();
        let space = store
            .create_space(&space_input(Some("Living room")))
            .unwrap();
        let planting = store
            .create_planting(&planting_input(profile, space))
            .unwrap();

        let mut afternoon = observation_input(planting, "repot");
        afternoon.occurred_on = crate::calendar::Date::new(2026, 10, 31).unwrap();
        afternoon.occurred_time = Some("15:00".into());
        afternoon.amount = Some(9.0);
        afternoon.unit = Some("L".into());
        let afternoon = store.create_observation(&afternoon).unwrap();

        let mut morning = observation_input(planting, "repot");
        morning.occurred_on = crate::calendar::Date::new(2026, 10, 31).unwrap();
        morning.occurred_time = Some("09:00".into());
        morning.amount = Some(5.0);
        morning.unit = Some("L".into());
        let morning = store.create_observation(&morning).unwrap();

        assert_eq!(
            store.observation(morning).unwrap().unwrap().previous_amount,
            Some(3.0)
        );
        assert_eq!(
            store
                .observation(afternoon)
                .unwrap()
                .unwrap()
                .previous_amount,
            Some(5.0)
        );
        assert_eq!(
            store
                .planting(planting)
                .unwrap()
                .unwrap()
                .planting
                .container_volume_l,
            Some(9.0)
        );

        // Removing the morning entry rewrites the afternoon one but leaves the volume.
        store.delete_observation(morning).unwrap();
        assert_eq!(
            store
                .observation(afternoon)
                .unwrap()
                .unwrap()
                .previous_amount,
            Some(3.0)
        );
        assert_eq!(
            store
                .planting(planting)
                .unwrap()
                .unwrap()
                .planting
                .container_volume_l,
            Some(9.0)
        );
    }

    #[test]
    fn keeps_a_space_that_another_planting_still_used() {
        let store = Store::open_in_memory().unwrap();
        let first_profile = store.create_profile("Chilli", None, None).unwrap();
        let second_profile = store.create_profile("Basil", None, None).unwrap();
        let shared = store.create_space(&space_input(Some("Grow tent"))).unwrap();
        let first = store
            .create_planting(&planting_input(first_profile, shared))
            .unwrap();
        let second = store
            .create_planting(&planting_input(second_profile, shared))
            .unwrap();
        let other = store.create_space(&space_input(Some("Room"))).unwrap();

        let mut moved = planting_input(first_profile, other);
        moved.moved_on = Some(crate::calendar::Date::new(2026, 10, 31).unwrap());
        store.update_planting(first, &moved).unwrap();

        // The old planting keeps a closed placement here, so the name stays required.
        let mut unnamed = space_input(None);
        unnamed.kind = "indoor".into();
        assert!(store.update_space(shared, &unnamed).is_err());

        // A database from before that rule can still hold a hidden space with this history.
        store
            .conn
            .execute("UPDATE spaces SET name = NULL WHERE id = ?1", [shared])
            .unwrap();

        // An already-private space stays editable, even with history from an earlier planting.
        assert!(store.update_space(shared, &unnamed).is_ok());

        store.delete_planting(second).unwrap();
        assert!(store.space(shared).unwrap().is_some());

        store.delete_planting(first).unwrap();
        assert!(store.space(shared).unwrap().is_none());
    }
}
