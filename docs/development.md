# Development

OmaGrow is a native GTK4/libadwaita app. There is no dev server and no hot reload: you edit,
rebuild and relaunch.

## The loop

Run from the repository root:

```sh
cargo run
```

The first build compiles the GTK stack once; after that a rebuild takes about a second.

1. Keep the app open and a terminal free.
2. Change code.
3. `Ctrl+C` in the terminal (or close the window), then `cargo run` again.

If the app is **already running**, a second launch is forwarded to the existing process and opens
another window. Stop it first:

```sh
pkill -x omagrow; cargo run
```

## Commands

| Command | What it does |
| --- | --- |
| `cargo check` | Typecheck only, no binary — the fastest feedback |
| `cargo build` | Build the debug binary |
| `cargo run` | Build and launch |
| `cargo test` | Unit tests for storage, validation and UI helpers |
| `cargo clippy --all-targets` | Lint the library, binary, tests and examples |
| `cargo run --release` | Optimised build; slower to compile, smoother to use |

The binary lands at `target/debug/omagrow`; running it directly works too.

## Inspecting the UI

```sh
GTK_DEBUG=interactive cargo run
```

Opens the GTK Inspector next to the app: widget tree, style classes and live CSS, so you can try a
style change without rebuilding.

## Interface conventions

- The sidebar is the top level. It starts with the grow — Plantings, Spaces, Plant Profiles — with
  Help and Settings at the bottom; the app opens on Plantings.
- An area always opens at its list. Leaving an area closes the pages pushed inside it; the back
  button goes one step up within the area.
- The active highlight follows the page, not the route: a planting opened from a space highlights
  Plantings, and going back to the space highlights Spaces again.
- Every form asks before it throws input away. Cancel, the back button, Escape, a sidebar click and
  the window close all use the same guard in `src/ui/shell.rs`; a new form page builds a `Guard`,
  while a page without input keeps the plain header.
- Chrome is symbolic and follows the Omarchy theme. The grow areas use plant-green icons: a seedling
  for Plantings, a potted plant for Spaces and a sprig for Plant Profiles; Help and Settings keep a
  neutral sidebar icon. An avatar is a picture, never a glyph; see [media.md](media.md).
- The Help page in the sidebar gives a short visual overview, the data credits and what an agent
  does; the app itself never launches one.
- Buttons and sidebar rows keep the arrow cursor; the pointing hand is for real links.

## Wide windows

A page body is built with `ui/shared/layout.rs`. Below a 1426 px window it is one reading column of
672 px, exactly as before. Above it, the body splits into a main column (712 px at the split point,
growing to about 734 px) and a rail of 400 px; it stays split down to 1386 px, where the main column
is still 672 px. That 40 px band is the hysteresis, and the decision is driven by the window width,
because the wide layout's own minimum width would keep the body from shrinking back.

The split has its own `GtkLayoutManager` in the same file, not a box. A box gives every child its
natural width before the rest goes to the expanding one, and a wrapping label's natural width
changes with the text of the selected day; that made the rail grow and the main column shrink on
every calendar click. The layout manager fixes both widths and measures the heights at those
widths.

A card is added with `columns.push(&widget, Slot::Main)`, `Slot::Side` or `Slot::Full`. `Full` spans
the whole content width below the two columns; on a narrow page it stays in the one-column order.
The planting page uses it for the Climate section; the space page does too. A page without a side
card stays narrow: the layout check must not rebuild it on a wide window, or a click on a row is
lost.

The planting page puts the title/hero over the full width above the columns. Below it the main
column holds the Grow timeline and the calendar; the rail holds the setup tiles, weather, diary and
notes, and the Climate section spans the full width below the columns. A planting owns no readings,
so its page has no readings card: the Climate section draws the readings of the space. The space page
puts the shared setup at the top of the main column, followed by the plantings; its rail holds one
card per source — the weather card (outdoor spaces), the readings card and the Sensors placeholder —
and the Climate section spans the full width below.
Every space has a side card, so every space page gets the two-column layout on a wide window. A list
does not split; it uses
`Columns::widening` to grow its one column (780 to 1000 px for the plantings and spaces lists) without
creating an empty rail. The Climate section picks by its own width, not the window's, because a
one-column page can sit in a wide window: from 900 px it shows the metrics as a 2×2 grid, below that
one graph with a tab per metric. Editors, Settings, history and the diary stay one column, and Help
has no rail either: each of its sections splits internally. A 21:9 window leaves room for a third
tier later.

A Help section puts its screenshot beside the text from 760 px of section width; below that the
screenshot moves under the text and shrinks with the page instead of being stretched. A small layout
manager in `src/ui/help.rs` makes that choice from the width it is given, so a wrapping label cannot
make the columns jump. The files and their sizes are in [media.md](media.md).

The window does not go below 520 px. The floor comes from the calendar header on the planting page:
"Calendar", both arrows, "November 2026" and "Today" on one line, and a label cannot shrink below its
full text. The declared minimum stays just above what the fullest page needs, so GTK never has to
warn that the content is wider than the window.

## Live theme test

Leave the app running and switch theme in another terminal:

```sh
omarchy theme set gruvbox
```

`src/theme.rs` maps `colors.toml` onto libadwaita's named colours and watches
`~/.local/state/omarchy/current`, so the open window follows a theme switch. This is the only live
reload the app has — and the easiest way to check that theming still works.

## Window behaviour

Hyprland tiles new windows. Float it (`Super+V`) or drag it when you want a fixed size while
working; tiling is the compositor, not the app.

## Optional: restart on save

```sh
cargo install cargo-watch
cargo watch -x run
```

It closes and reopens the window on every save. For UI work that is usually more disruptive than
running `cargo run` by hand.

## Running next to your own instance

Two instances of the same app id cannot run at once; the second launch just activates the first.
To test against a copy of the database while your own app keeps running:

```sh
mkdir -p /tmp/omagrow-test/omagrow
cp ~/.local/share/omagrow/omagrow.db /tmp/omagrow-test/omagrow/
OMAGROW_APP_ID=io.github.nicomdev.OmaGrow.Test XDG_DATA_HOME=/tmp/omagrow-test cargo run
```

## Logs

Panics and GTK warnings go to the terminal that started the app. `G_MESSAGES_DEBUG=all` adds GLib
debug output (verbose).

### Known GTK 4.24 frame-clock warning

On this setup (GTK 4.24.1, Hyprland, fractional scaling), opening a popover or resizing the window
can print:

```text
Gdk-WARNING **: gdk_frame_timings_presented() called on discarded frame
```

Variants mention a duplicate call with a presentation time off by a frame. The same warnings
appear in Nautilus, so this is a GTK/Wayland frame-clock warning rather than an OmaGrow bug. It is
cosmetic; the UI keeps working.

## Network

Data itself is local. The place search, frost estimate and weather sync are the only requests; their
rules, budget and licence are in [weather.md](weather.md), and the frost estimate in
[plant-data.md](plant-data.md). The live network test is ignored by default:

```sh
cargo test -- --ignored
```

## Packaging

There is no desktop entry or icon yet, so the app runs from the terminal. See
[roadmap.md](roadmap.md) for the plan.
