//! What a value is worth, in the user's eyes.
//!
//! An agent writes `estimated` values with a source. The user can confirm a value, unconfirm it
//! again so an agent may update it, or mark it as not applicable. Confirmed and not-applicable
//! values are protected by every agent write path.

use crate::plant_data::Provenance;
use crate::store::{self, Store};

/// The user confirms the current value, keeping the source that came with it.
pub fn confirm(
    store: &Store,
    entity_type: &str,
    entity_id: i64,
    fields: &[&str],
) -> store::Result<()> {
    for field in fields {
        let mut provenance = existing(store, entity_type, entity_id, field)?;
        provenance.status = "confirmed".to_string();
        if provenance.source_type.is_none() {
            provenance.source_type = Some("user".to_string());
        }
        store.set_provenance(&provenance)?;
    }

    Ok(())
}

/// Gives an agent access again. A value with a real source goes back to `estimated`; a value the
/// user entered themselves simply loses its status.
pub fn unconfirm(
    store: &Store,
    entity_type: &str,
    entity_id: i64,
    fields: &[&str],
) -> store::Result<()> {
    for field in fields {
        let Some(mut provenance) = store.provenance(entity_type, entity_id, field)? else {
            continue;
        };
        if provenance.status != "confirmed" {
            continue;
        }

        if provenance
            .source_type
            .as_deref()
            .is_some_and(|source| source != "user")
        {
            provenance.status = "estimated".to_string();
            store.set_provenance(&provenance)?;
        } else {
            store.delete_provenance(entity_type, entity_id, field)?;
        }
    }

    Ok(())
}

/// The plant explicitly has no value here. The value itself is left alone; the status is the
/// promise that an agent will not add one.
pub fn mark_not_applicable(
    store: &Store,
    entity_type: &str,
    entity_id: i64,
    fields: &[&str],
) -> store::Result<()> {
    for field in fields {
        let mut provenance = existing(store, entity_type, entity_id, field)?;
        provenance.status = "not_applicable".to_string();
        if provenance.source_type.is_none() {
            provenance.source_type = Some("user".to_string());
        }
        store.set_provenance(&provenance)?;
    }

    Ok(())
}

/// A value the user entered or changed in the app. The source becomes the user, not the agent.
pub fn mark_user(
    store: &Store,
    entity_type: &str,
    entity_id: i64,
    fields: &[&str],
) -> store::Result<()> {
    for field in fields {
        store.set_provenance(&Provenance {
            entity_type: entity_type.to_string(),
            entity_id,
            field: (*field).to_string(),
            status: "confirmed".to_string(),
            source_type: Some("user".to_string()),
            source_ref: None,
            retrieved_at: None,
            confidence: None,
            notes: None,
        })?;
    }

    Ok(())
}

fn existing(
    store: &Store,
    entity_type: &str,
    entity_id: i64,
    field: &str,
) -> store::Result<Provenance> {
    Ok(store
        .provenance(entity_type, entity_id, field)?
        .unwrap_or(Provenance {
            entity_type: entity_type.to_string(),
            entity_id,
            field: field.to_string(),
            status: "confirmed".to_string(),
            source_type: None,
            source_ref: None,
            retrieved_at: None,
            confidence: None,
            notes: None,
        }))
}

#[cfg(test)]
mod tests {
    use super::{confirm, mark_not_applicable, mark_user, unconfirm};
    use crate::plant_data::Provenance;
    use crate::store::Store;

    fn estimated(store: &Store) {
        store
            .set_provenance(&Provenance {
                entity_type: "profile".to_string(),
                entity_id: 1,
                field: "lifecycle".to_string(),
                status: "estimated".to_string(),
                source_type: Some("website".to_string()),
                source_ref: Some("https://example.org".to_string()),
                retrieved_at: Some("2026-10-29".to_string()),
                confidence: Some("high".to_string()),
                notes: None,
            })
            .unwrap();
    }

    #[test]
    fn confirming_keeps_the_source_and_unconfirming_restores_estimated() {
        let store = Store::open_in_memory().unwrap();
        let id = store.create_profile("Chilli", None, None).unwrap();
        estimated(&store);
        let _ = id;

        confirm(&store, "profile", 1, &["lifecycle"]).unwrap();
        let provenance = store
            .provenance("profile", 1, "lifecycle")
            .unwrap()
            .unwrap();
        assert_eq!(provenance.status, "confirmed");
        assert_eq!(provenance.source_type.as_deref(), Some("website"));

        unconfirm(&store, "profile", 1, &["lifecycle"]).unwrap();
        let provenance = store
            .provenance("profile", 1, "lifecycle")
            .unwrap()
            .unwrap();
        assert_eq!(provenance.status, "estimated");
    }

    #[test]
    fn unconfirming_a_user_value_removes_the_status() {
        let store = Store::open_in_memory().unwrap();
        let id = store.create_profile("Chilli", None, None).unwrap();

        mark_user(&store, "profile", id, &["lifecycle"]).unwrap();
        unconfirm(&store, "profile", id, &["lifecycle"]).unwrap();
        assert!(
            store
                .provenance("profile", id, "lifecycle")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn not_applicable_is_a_status_and_can_be_cleared() {
        let store = Store::open_in_memory().unwrap();
        let id = store.create_profile("Chilli", None, None).unwrap();

        mark_not_applicable(&store, "profile", id, &["determinacy"]).unwrap();
        let provenance = store
            .provenance("profile", id, "determinacy")
            .unwrap()
            .unwrap();
        assert_eq!(provenance.status, "not_applicable");

        confirm(&store, "profile", id, &["determinacy"]).unwrap();
        assert_eq!(
            store
                .provenance("profile", id, "determinacy")
                .unwrap()
                .unwrap()
                .status,
            "confirmed"
        );
    }
}
