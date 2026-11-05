# Media

Photos are files next to the database, never blobs in SQLite. The `media` table stores the owner,
the role and the stem of a file pair; the pixels live under the data directory.

## Layout

```text
~/.local/share/omagrow/
  omagrow.db
  media/
    profile-5-cover-large.jpg          max 2048 px on the longest side
    profile-5-cover-thumb.jpg          max 320 px, for cards and lists
    planting-3-avatar-large.jpg        the planting's own avatar
    planting-3-avatar-thumb.jpg
    space-2-avatar-large.jpg           the space's own avatar
    space-2-avatar-thumb.jpg
    observation-12-photo-large.jpg     a diary entry's photo
    observation-12-photo-thumb.jpg
```

The stem is `<owner_type>-<owner_id>-<role>` by convention (`media::stem_for`), so the files are
recognisable without opening the database. Runs of letters, digits, dashes and underscores only:
`validate_media` rejects anything else, because the stem becomes a file name. Profile, planting and
observation ids are `AUTOINCREMENT`, so a deleted id and its stem are never reused.

The `media` row carries `owner_type`, `owner_id`, `role`, `file_stem`, `width`, `height` and
`bytes`; `bytes` is the two files together, and `width`/`height` describe the large copy. There is
no unique index on the owner and role: a cover is one row today, and a gallery later can add more
rows with role `photo`. `Store::set_media` updates the first row for an owner and role — that is a
cover helper, not a gallery insert.

Owners follow the plant data layers: `profile` for a cover, `planting` and `space` for an avatar,
`observation` for a diary photo. A camera frame later becomes an observation with a photo row, so
retention can be a per-observation policy instead of a second storage system.

## Avatars and illustrations

A profile, planting and space each own one avatar. The app shows, in order: the chosen illustration,
the owner's own photo, then a fallback illustration for that owner. A planting never borrows the
profile photo: the user may grow one variety in several spaces and want a different picture in each.

There are two icon languages, and they do not mix. Chrome — the sidebar, buttons, chips, weather and
menus — is symbolic and follows the theme. An avatar is a picture: a photo or an illustration, never
a glyph. The crop and place pictures keep their own colours; they are content, not controls.

Without an illustration, the profile cover is a wide photo on the card and the hero, and its
thumbnail is the profile avatar. When an illustration is chosen it replaces that cover band and the
hero, while the photo stays stored until the user picks it again. With neither, the hero and the
card band show the owner's fallback picture. Plantings and spaces have no wide cover, only the
square avatar.

The chosen illustration lives in `avatar_icons` (`owner_type`, `owner_id`, `icon`): `crop:<slug>`
for a crop, `space:<slug>` for a place. It is a name, not a file, so replacing the picker's lists
later does not migrate the stored rows. Rows written by the old picker hold a symbolic icon name or,
for a space, a crop; the picker no longer offers them, but they keep rendering. An unknown name
falls back to the photo or the owner's fallback instead of breaking the avatar.

The picker follows the owner. A profile or planting gets the searchable crop grid; a space gets the
twelve place illustrations. The picker never offers the other set, so a new space avatar is a place
and a new planting avatar is a crop. A choice stored by the old shared picker still renders as it
was; the picker no longer offers it. **Use photo or default** clears the choice.

The crop set is the full [Open Crop Icons](https://github.com/openfarmcc/open-crop-icons) set, 247
crop SVGs under CC0, plus a seed packet drawn for OmaGrow. The 248 files live in `assets/crops` and
are bundled as `/io/github/nicomdev/OmaGrow/crops/<slug>.svg`; they are marked `compressed` in the
gresource, which keeps the app around a megabyte larger instead of more than two. The crop search
filters the grid. The images are rasterized lazily after the dialog opens, in small chunks, so a
large set does not block it.

The place set is twelve illustrations drawn for OmaGrow: garden bed, raised bed, balcony, patio,
orchard, greenhouse, polytunnel, cold frame, windowsill, indoor shelf, grow tent and hydroponics.
They live in `assets/spaces` and use the resource path
`/io/github/nicomdev/OmaGrow/spaces/<slug>.svg`. They follow the crop recipe — flat fills, a warm
material-coloured outline and a highlight — so they sit beside the crops without becoming UI icons.

The fallbacks are pictures too: a profile shows the seed packet crop, a planting shows
`neutral/sprout.svg`, and a space shows the place that matches its kind (`garden-bed` for outdoor,
`greenhouse` for greenhouse, `indoor-shelf` for indoor). All illustrations are rasterized at 2× and
displayed at the logical size, so they stay sharp on HiDPI screens.

## Help screenshots

The Help page shows five cropped screenshots from `assets/help/`:

| File | Shows |
| --- | --- |
| `hero.png` | The planting list: space, stage and day count |
| `profiles.png` | Day and night temperature ranges and the growing cycle |
| `plantings.png` | The calendar with a selected day and its entry below |
| `spaces.png` | An indoor space's shared light, plantings and readings |
| `agent.png` | A profile's activity list, with a revert on every change |

They were captured from a local app instance at 2× and show interface only: no sidebar, window
buttons, free-text notes or location. Refresh them when the corresponding screens change; crop one
section that tells the story, not the whole window.

Each file is a 3:2 crop, 870–1000 px wide, and is shown at up to 400 px: the card keeps that width
and the image shrinks with a narrower page, never stretched. The app bundles the PNG and lets GTK
scale the texture to the card; it must not pre-scale the image to the display size, because that
softens it again on a HiDPI screen. The layout lives in [development.md](development.md).

## Import

`src/media.rs` reads the file with gdk-pixbuf, follows its EXIF orientation, then writes two JPEGs:

- **large** at most 2048 px on the longest side, quality 90 — the detail view, the hero, and the
  copy an agent can inspect;
- **thumb** at most 320 px, quality 90 — the cards and the diary row.

The original upload is not kept. A phone photo of 8 MB becomes a large copy of about a megabyte
and a thumbnail of 5–20 kB, which keeps the data directory predictable and the backup small. The
large copy is enough for a full-screen view and for inspecting a leaf or a pest; it is not the
original. If originals ever matter, they belong in a separate "keep original" setting, not in the
default path. A card or row uses the thumbnail; the photo viewer and an agent use the large copy —
never stretch a thumbnail into a detail view.

The decoder asks for a reduced decode at the target size, so a 48 MP photo does not sit in memory
at full resolution. The thumbnail is derived from the large copy, not from the source: 320 px from
2048 px is plenty and keeps the pipeline short. A transparent PNG is flattened onto white first,
because gdk-pixbuf turns an RGBA pixbuf into a greyscale JPEG. Small images are not blown up: a
200 px photo stays 200 px in both copies.

New files are written as `<stem>-large.jpg.tmp` and `<stem>-thumb.jpg.tmp` and renamed into place
only when both are complete, so a failed import leaves the previous photo intact. Replacing a cover
reuses the stem and overwrites the pair.

`Store::delete_profile` and `Store::delete_observation` remove the rows but cannot touch the files;
the delete actions in the UI read the stems first and call `media::remove` after the rows are gone.
Deleting a planting removes the rows of its diary photos and its own avatar and returns their stems,
so the caller removes the files after the cascade. It also removes the photos and avatar rows of a
private space that it actually deletes; a private space kept for another planting's closed placement
keeps its rows and files. A future caller that deletes an owner must do the same. The hero and the
cards tolerate a missing file by falling back to the placeholder or to the large copy.

## Rules

- Never write into `media/` from anywhere but `src/media.rs`; the files and the row have to change
  together. Today that coupling lives in the photo callbacks in `src/ui.rs`, the avatar editor in
  `src/ui/shared/avatar.rs`, `src/ui/planting_diary.rs` and the planting delete in
  `src/ui/planting_view.rs`; an import path or a gallery should own the same sequence rather than
  copying it.
- Store stems, not absolute paths: a moved data directory must keep working.
- One row per owner and role is a UI rule, not a schema rule.
- The CLI does not expose media yet; when it does, it should return the stem and the dimensions of
  the large copy, never base64 pixels.
