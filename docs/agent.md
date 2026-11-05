# Agent access

OmaGrow is meant to be filled in with help from a coding agent. The app and the agent meet at a
small CLI in the same binary: the agent reads JSON, writes through the app's own validation, and
never touches SQLite. A proposal file remains available for review-first work, batch imports and
sharing.

## Two directions

- **You → agent.** You open your own agent and tell it what you want to do. OmaGrow has no agent
  button and does not start one; the Help page points at `omagrow agent guide`. Ask the agent to
  run that once so it knows the contract.
- **Agent → app.** An agent only knows about OmaGrow when it can find the contract. It ships as
  `assets/agent/SKILL.md` and `omagrow agent guide` prints the same text without the frontmatter.
  Installing it under `~/.agents/skills/omagrow/` (the path Omarchy already symlinks into OpenCode,
  Claude and Codex) is a later, opt-in step.

## Commands

```sh
omagrow profile list        # every profile, with id, name, species and cultivar
omagrow profile show <id>   # one profile with details, stages, requirements, timing, attributes
omagrow profile create      # new profile; --name, --species, --cultivar
omagrow profile update <id> # change identity; --name, --species, --cultivar
omagrow profile copy <id>   # copy plant data and provenance; optional identity flags
omagrow attributes list     # the attribute registry: key, category, type, unit, enum values
omagrow set <id>            # write a JSON document from stdin; --check validates only
omagrow import <file>       # apply a proposal file; --check validates without writing
omagrow agent guide         # the contract
omagrow --help              # usage
```

The data commands print compact JSON on stdout; there is no human table. `agent guide` prints the
contract as markdown and `--help` prints plain usage.

`list` and `show` open the database read-only (`SQLITE_OPEN_READ_ONLY`): they do not create the
file, set WAL, migrate or seed. A missing database is an error that tells the user to open the app
once, and a read-only database file still works. A fully read-only directory does not: SQLite needs
to create the `-shm` and `-wal` side files for a WAL database, so the directory must be writable. A
schema other than the one this build expects is refused with the same advice. Reading works while
the app is open.

`profile create`, `profile update`, `profile copy`, `set` and `import` open the database
read-write, with foreign keys on, and need a writable directory. `--check` uses the same handle
because it validates by writing and rolling back. There is no delete command: only the user can
delete a profile in the app.

Exit codes: `0` success, `1` no database or a lookup error, `2` usage error. Errors go to stderr;
output never goes to stderr and errors never go to stdout.

## JSON shape

```text
{
  "api": 1,                       // shape version, bumped when agents must notice a change
  "app": "omagrow",
  "app_version": "0.1.0",
  "guide": "omagrow agent guide",
  "vocabulary": { … },            // every closed value set and the unit per metric
  "profile": {
    "id": 5,
    "name": "Ghost Pepper",
    "species": "Capsicum chinense",
    "cultivar": "Bhut Jolokia Red",
    "details": { … },             // core traits, null when unknown
    "provenance": { "family": { "status": "estimated", … } },
    "stages": [ { "id", "position", "kind", "label", "duration_days_min", "duration_days_max",
                  "notes", "repeats", "provenance" } ],
    "requirements": [ { "id", "stage_id", "metric", "day_part", "min", … } ],
    "timing_rules": [ { "id", "stage_id", "kind", "anchor", "offset_min_days", … } ],
    "attributes": [ { "id", "key", "value_type", "value_num", …, "provenance" } ]
  }
}
```

`null` means unknown. A requirement with `stage_id: null` is the profile default; a stage row
overrides it, and a `not_applicable` provenance row overrides both. Provenance per row is the `*`
row: status, source type, source reference, retrieval date, confidence and notes.

`omagrow attributes list` returns the registry so an agent can pick a key before proposing a
long-tail value:

```json
{"api":1,"definitions":[{"key":"climate_type","label":"Climate type","description":"…",
  "value_type":"enum","unit":null,"category":"Climate","applies_to":"profile",
  "allowed_stages":null,"allowed_lifecycles":null,"enum_values":["tropical","subtropical","…"],
  "is_custom":false}]}
```

## Writing

Identity is changed with `omagrow profile update`, not through `set`: the `detail` target
deliberately covers only the fields under `profile.details`. `profile copy` copies the plant data
and provenance (not photos or history), and accepts `--name`, `--species` and `--cultivar` to set
the copy's identity in the same step. `omagrow set <id>` reads a JSON document from stdin and
writes it; `omagrow import <file>` does the same for a file.

Every profile value write leaves a row in the change history, except a provenance status change or
a stage reorder. Activity shows the newest changes with the value, History groups them by day, and
one click reverts a row. That is what makes direct writes acceptable: the check moved from before
the write to after it ([docs/history.md](history.md)). A proposal saved in `~/.local/share/omagrow/proposals/` also shows
up on the profile page as **Proposals → Review**: the same document, with a tick per value and the
source next to it, and the file moves to `proposals/applied/` after applying. Confirmed and
not-applicable values are never overwritten; a stage can be reordered with a 1-based `position`
without deleting it. An open profile editor notices a write from an agent and offers a reload instead
of rebuilding the page under the user's cursor.

The format, targets and guard rails below are shared by `set`, `import` and the review screen.

### The document

```json
{
  "api": 1,
  "profile": 5,
  "values": [
    {
      "target": "detail",
      "field": "lifecycle",
      "value": "perennial",
      "source_type": "website",
      "source_ref": "https://example.org/ghost-pepper",
      "confidence": "high",
      "retrieved_at": "2026-10-28",
      "notes": "optional free text"
    },
    {
      "target": "stage",
      "label": "Flowering",
      "kind": "flowering",
      "duration_days_min": 30,
      "duration_days_max": 45,
      "repeats": true,
      "position": 4
    },
    {
      "target": "requirement",
      "stage": "Flowering",
      "metric": "air_temp",
      "day_part": "day",
      "min": 12,
      "max": 32,
      "opt_min": 22,
      "opt_max": 28,
      "survival_min": 0,
      "survival_max": 38,
      "source_type": "website",
      "source_ref": "https://example.org/temperature"
    },
    {
      "target": "timing_rule",
      "stage": null,
      "kind": "sow_indoor",
      "anchor": "last_frost",
      "offset_min_days": -70,
      "offset_max_days": -56,
      "source_type": "book",
      "source_ref": "RHS encyclopedia, p. 212"
    },
    {
      "target": "attribute",
      "stage": null,
      "key": "climate_type",
      "value": "tropical",
      "source_type": "website",
      "source_ref": "https://example.org/climate"
    }
  ]
}
```

Field names and units match `omagrow profile show`, so an agent reads and writes one vocabulary.
The review screen may display values in the user's metric or imperial preference; the file itself
stays canonical metric.

### Targets

| Target | Writes | Notes |
| --- | --- | --- |
| `detail` | One core trait of the profile | The fields under `profile.details`; use `omagrow profile update` for name, species and cultivar. |
| `stage` | A stage | Matched by label (case-insensitive). A new stage is appended; an optional 1-based `position` moves the cycle into order after every stage exists. `repeats: true` marks a perennial stage for the yearly cycle, `false` keeps it in the first cycle only; omit or `null` to leave the stored value alone (a new stage then follows the kind-derived rule). |
| `requirement` | One range per stage, metric and day part | `stage: null` is the profile default. Replaces the existing row. |
| `timing_rule` | One relative timing rule | Replaces the rule with the same stage and kind. |
| `attribute` | One long-tail value | The key must exist in the registry; `enum` values must be listed. |

### Sources

A requirement may set `"override_confirmed": true`. Use it only after the user agrees that a
stage- or day-specific row may replace their confirmed value for that scope; without it the write
is refused. A requirement the user marked `not_applicable` is never overridden by a proposal;
only the user can change that in the app.

`source_type` is one of `agent`, `import`, `label`, `book`, `website`. A value needs a
`source_ref` (URL, book title, label) for every type except `agent`. `user` is the app's own mark
for a value the user confirmed; a proposal cannot claim it. `confidence` is `low`, `medium` or
`high`; a value from the model's own knowledge is always stored as `low`, whatever the document
says.

The user chooses the policy in **Settings → Agent sources**:

- **Sources only** (default): a value without a cited source is refused, including `agent`.
- **Model knowledge**: the model may fall back when it found nothing. Those values use
  `source_type: "agent"`, are always `low` confidence, and show up in the app as model knowledge.

## Applying a document

```sh
omagrow import proposal.json --check   # validate and roll back; prints the change list
omagrow import proposal.json           # apply
```

`--check` runs the real writes in a transaction and rolls it back, so it needs a writable database
just like `apply`; it changes nothing on disk. `set` takes the same document on stdin.

Both print JSON: the profile, whether it was checked or applied, and one line per change. Errors go
to stderr with exit code 1 and name the value that failed (`value 3: ...`).

The whole file applies or nothing does: the changes run in one transaction, so a failure halfway
rolls the earlier ones back too.

## Rules the code enforces

- A proposal cannot delete a stage. A stage with requirements, timing or attributes cannot be
  deleted in the app either until those are removed; use `position` to reorder instead.
- Unknown profile, target, field, metric, day part, stage kind, timing kind, anchor, attribute key,
  source type or confidence is an error naming the allowed values.
- A requirement needs at least one bound and a sane order
  (`survival_min ≤ min ≤ opt_min ≤ opt_max ≤ max ≤ survival_max`); the store validates this.
- A value the user confirmed, or marked not applicable, is never changed; the import stops with an
  error naming the value.
- A stage from another profile is not found; stage labels resolve inside the proposal's profile.
  A stage may be declared after the value that uses it.
- A proposal never changes the profile name, species or cultivar.
- Applying the same proposal twice is safe: requirements, attributes and timing rules replace their
  match, and a stage label maps to the same stage.
- Bounds and durations a proposal does not name are kept; a proposal cannot clear a value. Clearing
  is a user action in the app.
- The source policy is checked again when the proposal is applied, not only when it is read.

## Review in the app

The profile page scans `~/.local/share/omagrow/proposals/` for files that name the profile and
offers each one as **Proposals → Review** when you open or return to it; there is no live watcher. The review page lists every change with its source and a tick; accepted changes go
through the same validation and transaction as `import`.

When every value was accepted, the file moves to `proposals/applied/` (a second file with the same
name gets a number). A partial apply leaves the file in the inbox so the rejected values can still
be reviewed. `omagrow import` archives a file that came from the inbox in the same way.

The CLI path stays for users who ask an agent to apply a proposal directly; the app path is the one
where every value is seen first. Direct writes are the normal path (`omagrow set`, with the change
history as the safety net). A proposal file is still useful when the user wants to see every value
before it is written, and for batch work or offline use.

## Development

```sh
cargo run -- profile list
cargo run -- profile show 5
cargo run -- agent guide
```

The binary is `target/debug/omagrow`; commands never start GTK, so they work without a display.
For a scratch database, set `XDG_DATA_HOME`:

```sh
XDG_DATA_HOME=/tmp/omagrow-cli target/debug/omagrow profile list
```
