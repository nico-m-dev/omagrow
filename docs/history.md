# Change history

Every profile value write OmaGrow keeps leaves a history row: what changed, from which value to
which, who did it and where it came from. The profile page shows the five newest under **Activity**
with the value in the row; **History** groups every change by day with a revert per row. The ledger
is profile-scoped: planting setup writes are not in it (the diary is the planting's own record).
Diary entries are facts, not editable values; deleting one is a user action with a confirmation,
and a deleted repot does not roll the current pot size back.

The point is not bookkeeping. It is what lets an agent write directly: the safety moves from
approving a proposal before the write to seeing and undoing it afterwards.

## The row

```text
changes
  id
  profile_id     -- filtering and cleanup are per profile
  entity_type    -- profile, stage, requirement, timing_rule, attribute
  entity_id
  field          -- a detail column, or * for a whole row
  old_value      -- JSON, null when the value did not exist
  new_value      -- JSON, null when the value was removed
  source_type    -- website, book, agent, user, ...
  source_ref
  actor          -- user or agent
  created_at
```

- A **detail** change is one row per field (`name`, `species`, `lifecycle`, `days_to_maturity_min`,
  …), so the history reads like a diff. Identity fields are included.
- A **row** change (stage, requirement, timing rule, attribute) stores the whole row as JSON with
  `field = '*'`. One revert puts the values back, including bounds or the stage. A stage keeps its
  current position in the cycle: reordering is not part of the value revert.
- Activity and History both show the value: `12 → 14` for a detail, `optimum 20–26 °C` for a
  requirement, `Ripening · 28–35 days` for a stage, and the text value for an attribute. Metric or
  imperial is only a display preference; the stored `old_value` and `new_value` stay canonical
  metric. The source is shortened to its domain in the row and kept in full in the tooltip.
- The proposal import writes its history rows inside the same transaction as the values, so a
  failed apply leaves neither values nor history.
- A profile delete removes its history.

## Revert

Revert writes the old value back through the same validation as any other write, and records the
revert as a new history row with actor `user`.

The restored value is marked **confirmed** with source `user`, because the user chose it. Reverting
back to an empty value removes its provenance. This prevents an old agent source from staying on a
value the user just restored.

It refuses when the value changed again after the change being reverted: the current value no
longer matches the `new_value` of that history row. For a stage, the position is ignored in that
comparison. That turns the list into a safe stack instead of a blind undo. Reverting a stage that
still has its own requirements, timing rules or attributes is refused as well; those rows have
their own history.

## What is recorded today

- Every agent write, through `omagrow set`, `omagrow import` or the proposal review: details,
  stages, requirements, timing rules and attributes.
- Every save and delete in the profile detail form and in the stage, requirement, timing and
  attribute editors, compared before and after. Identity changes are included.
- A stage delete is refused while the stage still has requirements, timing or attributes, so a
  revert cannot silently bring back an empty stage.

Not in the ledger yet: stage reordering and provenance status changes (confirm, unconfirm,
not applicable). They are visible on the page, but they do not create a history row of their own.

A profile delete removes its history with it; there is nothing to revert after that.

Planting setup, observations and readings are not in this ledger; [roadmap.md](roadmap.md) tracks
that gap. If actuator control is ever tested, it would need the same audit trail, but it is a future
experiment, not a release feature.
