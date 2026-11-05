//! One avatar for a profile, planting or space: a photo, a chosen illustration or a fallback.
//!
//! The owner is independent. A planting never borrows the profile's photo, because the user may
//! grow one variety in several spaces and want a different picture in each.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use gdk_pixbuf::{InterpType, Pixbuf};
use gtk::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::media::{self, Media};
use crate::store::Store;

use super::crops::{CROPS, label as crop_label};
use super::spaces::{SPACES, default_for_kind, label as space_label};

/// Symbolic avatars stored before the illustration sets existed. They are no longer offered, but
/// they keep rendering so an old row does not lose its picture.
const LEGACY_ICONS: &[&str] = &[
    "omagrow-plant-symbolic",
    "omagrow-planting-symbolic",
    "omagrow-space-symbolic",
    "emoji-nature-symbolic",
    "weather-clear-symbolic",
    "weather-showers-symbolic",
    "applications-science-symbolic",
    "view-grid-symbolic",
    "user-home-symbolic",
];

/// A bundled illustration. The set follows the owner: crops for a profile or planting, a place
/// for a space, and the sprout for a planting that has not chosen anything yet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::ui) enum Picture {
    Crop(&'static str),
    Space(&'static str),
    Neutral(&'static str),
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::ui) enum Source {
    Icon(&'static str),
    /// An illustration the user chose.
    Picture(Picture),
    Photo(String),
    /// The owner's fallback illustration: a neutral picture, not another owner's.
    Default(Picture),
}

pub(in crate::ui) fn photo_role(owner_type: &str) -> &'static str {
    if owner_type == "profile" {
        "cover"
    } else {
        "avatar"
    }
}

/// A stored `crop:<slug>` or `space:<slug>` that still exists in its set.
fn picture_for_key(key: &str) -> Option<Picture> {
    if let Some(slug) = key.strip_prefix("crop:") {
        return CROPS
            .iter()
            .copied()
            .find(|name| *name == slug)
            .map(Picture::Crop);
    }
    if let Some(slug) = key.strip_prefix("space:") {
        return SPACES
            .iter()
            .copied()
            .find(|name| *name == slug)
            .map(Picture::Space);
    }
    None
}

/// The fallback depends on the owner, not on what another owner chose.
fn fallback_picture(owner_type: &str, kind: Option<&str>) -> Picture {
    match owner_type {
        "planting" => Picture::Neutral("sprout"),
        "space" => Picture::Space(default_for_kind(kind)),
        _ => Picture::Crop("seed-packet"),
    }
}

/// The picture an owner shows when it has no illustration and no photo.
pub(in crate::ui) fn fallback(store: &Store, owner_type: &str, owner_id: i64) -> Picture {
    let kind = if owner_type == "space" {
        store.space(owner_id).ok().flatten().map(|space| space.kind)
    } else {
        None
    };
    fallback_picture(owner_type, kind.as_deref())
}

/// The chosen illustration, then the owner's own photo, then the owner's fallback. Never another
/// owner's picture.
pub(in crate::ui) fn source(store: &Store, owner_type: &str, owner_id: i64) -> Source {
    if let Some(icon) = store.avatar_icon(owner_type, owner_id).ok().flatten() {
        if let Some(picture) = picture_for_key(&icon) {
            return Source::Picture(picture);
        }
        if let Some(name) = LEGACY_ICONS.iter().copied().find(|name| *name == icon)
            && icon_exists(name)
        {
            return Source::Icon(name);
        }
    }

    if let Some(media) = store
        .media(owner_type, owner_id, photo_role(owner_type))
        .ok()
        .flatten()
        && photo_path(&media.file_stem).is_some()
    {
        return Source::Photo(media.file_stem);
    }

    Source::Default(fallback(store, owner_type, owner_id))
}

/// A square avatar at `size`, with the same frame the existing lists used.
pub(in crate::ui) fn widget(
    store: &Store,
    owner_type: &str,
    owner_id: i64,
    size: i32,
) -> gtk::Widget {
    let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
    frame.set_size_request(size, size);
    frame.set_halign(gtk::Align::Center);
    frame.set_valign(gtk::Align::Center);
    frame.set_overflow(gtk::Overflow::Hidden);
    frame.add_css_class("omagrow-thumb");

    match source(store, owner_type, owner_id) {
        Source::Icon(name) => {
            let icon = icon_widget(name, (f64::from(size) * 0.5) as i32);
            icon.set_halign(gtk::Align::Center);
            icon.set_valign(gtk::Align::Center);
            icon.set_vexpand(true);
            frame.append(&icon);
        }
        Source::Picture(picture) | Source::Default(picture) => {
            let picture = picture_widget(picture, (size - 8).max(1));
            picture.set_halign(gtk::Align::Center);
            picture.set_valign(gtk::Align::Center);
            picture.set_vexpand(true);
            frame.append(&picture);
        }
        Source::Photo(stem) => {
            if let Some(path) = photo_path(&stem)
                && let Some(pixbuf) = avatar_pixbuf(&path, size)
            {
                let texture = gtk::gdk::Texture::for_pixbuf(&pixbuf);
                let picture = gtk::Picture::for_paintable(&texture);
                picture.set_content_fit(gtk::ContentFit::Cover);
                picture.set_can_shrink(true);
                picture.set_size_request(size, size);
                frame.append(&picture);
            } else {
                let picture =
                    picture_widget(fallback(store, owner_type, owner_id), (size - 8).max(1));
                picture.set_halign(gtk::Align::Center);
                picture.set_valign(gtk::Align::Center);
                picture.set_vexpand(true);
                frame.append(&picture);
            }
        }
    }

    frame.upcast()
}

/// An avatar that opens its own small editor when clicked. Used on detail pages, never in a list
/// where the whole row already has an action.
pub(in crate::ui) fn editor(
    store: &Rc<Store>,
    owner_type: &'static str,
    owner_id: i64,
    size: i32,
    overlay: &adw::ToastOverlay,
) -> gtk::MenuButton {
    let button = gtk::MenuButton::new();
    button.add_css_class("flat");
    button.set_focusable(true);
    button.set_tooltip_text(Some("Change avatar"));
    button.set_child(Some(&widget(store, owner_type, owner_id, size)));

    let popover = gtk::Popover::new();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 2);
    content.set_margin_top(6);
    content.set_margin_bottom(6);
    content.set_margin_start(6);
    content.set_margin_end(6);

    let view = menu_button("Show photo");
    let change = menu_button("Add photo…");
    let remove = menu_button("Remove photo");
    let icon = menu_button("Choose illustration…");
    let clear = menu_button("Use photo or default");

    content.append(&view);
    content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    content.append(&change);
    content.append(&remove);
    content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    content.append(&icon);
    content.append(&clear);
    popover.set_child(Some(&content));
    button.set_popover(Some(&popover));

    // The avatar and its menu live inside the page tree. Weak handles keep the update callback
    // from pinning that tree after a page rebuild.
    let update: Rc<dyn Fn()> = Rc::new({
        let button = button.downgrade();
        let view = view.downgrade();
        let change = change.downgrade();
        let remove = remove.downgrade();
        let clear = clear.downgrade();
        let store = store.clone();
        move || {
            let Some(button) = button.upgrade() else {
                return;
            };
            let (Some(view), Some(change), Some(remove), Some(clear)) = (
                view.upgrade(),
                change.upgrade(),
                remove.upgrade(),
                clear.upgrade(),
            ) else {
                return;
            };

            button.set_child(Some(&widget(&store, owner_type, owner_id, size)));
            let has_photo = store
                .media(owner_type, owner_id, photo_role(owner_type))
                .ok()
                .flatten()
                .is_some();
            view.set_visible(has_photo);
            change.set_label(if has_photo {
                "Change photo…"
            } else {
                "Add photo…"
            });
            remove.set_visible(has_photo);
            clear.set_visible(
                store
                    .avatar_icon(owner_type, owner_id)
                    .ok()
                    .flatten()
                    .is_some(),
            );
        }
    });
    update();

    view.connect_clicked({
        let store = store.clone();
        let menu = button.downgrade();
        move |trigger| {
            if let Some(menu) = menu.upgrade() {
                menu.popdown();
            }
            let Some(media) = store
                .media(owner_type, owner_id, photo_role(owner_type))
                .ok()
                .flatten()
            else {
                return;
            };
            let Some(path) = media::detail_path(&media.file_stem) else {
                return;
            };
            let Some(window) = trigger
                .clone()
                .upcast::<gtk::Widget>()
                .root()
                .and_then(|root| root.downcast::<gtk::Window>().ok())
            else {
                return;
            };
            crate::ui::show_photo(&window, &path);
        }
    });

    change.connect_clicked({
        let store = store.clone();
        let overlay = overlay.downgrade();
        let update = update.clone();
        let menu = button.downgrade();
        move |trigger| {
            let Some(menu) = menu.upgrade() else {
                return;
            };
            menu.popdown();
            let Some(overlay) = overlay.upgrade() else {
                return;
            };
            choose_photo(
                trigger,
                &store,
                owner_type,
                owner_id,
                &overlay,
                update.clone(),
            );
        }
    });

    remove.connect_clicked({
        let store = store.clone();
        let overlay = overlay.downgrade();
        let update = update.clone();
        let menu = button.downgrade();
        move |trigger| {
            let Some(menu) = menu.upgrade() else {
                return;
            };
            menu.popdown();
            let Some(overlay) = overlay.upgrade() else {
                return;
            };
            remove_photo(
                trigger,
                &store,
                owner_type,
                owner_id,
                &overlay,
                update.clone(),
            );
        }
    });

    icon.connect_clicked({
        let store = store.clone();
        let update = update.clone();
        let menu = button.downgrade();
        move |trigger| {
            let Some(menu) = menu.upgrade() else {
                return;
            };
            menu.popdown();
            choose_illustration(trigger, &store, owner_type, owner_id, update.clone());
        }
    });

    clear.connect_clicked({
        let store = store.clone();
        let overlay = overlay.downgrade();
        let update = update.clone();
        let menu = button.downgrade();
        move |_| {
            if let Some(menu) = menu.upgrade() {
                menu.popdown();
            }
            if let Err(error) = store.set_avatar_icon(owner_type, owner_id, None) {
                eprintln!("could not clear the avatar icon: {error}");
                if let Some(overlay) = overlay.upgrade() {
                    overlay.add_toast(adw::Toast::new("Could not change the avatar"));
                }
                return;
            }
            update();
        }
    });

    button
}

/// Opens the illustration picker for this owner. The callback refreshes the avatar that opened it.
pub(in crate::ui) fn choose_illustration(
    trigger: &impl IsA<gtk::Widget>,
    store: &Rc<Store>,
    owner_type: &'static str,
    owner_id: i64,
    refresh: Rc<dyn Fn()>,
) {
    let Some(window) = trigger
        .clone()
        .upcast::<gtk::Widget>()
        .root()
        .and_then(|root| root.downcast::<gtk::Window>().ok())
    else {
        return;
    };

    let dialog = adw::Dialog::builder()
        .title("Choose an illustration")
        .content_width(460)
        .content_height(560)
        .build();
    let overlay = adw::ToastOverlay::new();

    // The crop list is rasterized in chunks over idle turns; stop as soon as the dialog closes
    // so a cancelled picker does not keep working through the whole set.
    let loading = Rc::new(Cell::new(true));
    let loading_guard = loading.clone();
    dialog.connect_closed(move |_| {
        loading_guard.set(false);
    });

    let save: Rc<dyn Fn(String)> = Rc::new({
        let store = Rc::clone(store);
        let refresh = refresh.clone();
        let dialog = dialog.downgrade();
        let overlay = overlay.downgrade();
        move |key| {
            if let Err(error) = store.set_avatar_icon(owner_type, owner_id, Some(&key)) {
                eprintln!("could not store the avatar icon: {error}");
                if let Some(overlay) = overlay.upgrade() {
                    overlay.add_toast(adw::Toast::new("Could not save the avatar"));
                }
                return;
            }
            refresh();
            if let Some(dialog) = dialog.upgrade() {
                dialog.close();
            }
        }
    });

    let choices = picker_flow(3);
    let mut search = None;

    if owner_type == "space" {
        for slug in SPACES.iter().copied() {
            let choice =
                choice_button(picture_widget(Picture::Space(slug), 48), &space_label(slug));
            connect_choice(&choice, format!("space:{slug}"), save.clone());
            choices.append(&choice);
        }
    } else {
        let pending: Rc<RefCell<VecDeque<(&'static str, gtk::Box)>>> =
            Rc::new(RefCell::new(VecDeque::new()));
        for slug in CROPS.iter().copied() {
            let label = crop_label(slug);
            let (choice, preview) = choice_shell(&label);
            pending.borrow_mut().push_back((slug, preview));
            connect_choice(&choice, format!("crop:{slug}"), save.clone());
            choices.append(&choice);
        }

        let entry = gtk::SearchEntry::new();
        entry.set_placeholder_text(Some("Search crops"));
        entry.set_margin_start(12);
        entry.set_margin_end(12);
        entry.set_margin_top(6);
        let searchable: Rc<Vec<String>> = Rc::new(
            CROPS
                .iter()
                .map(|slug| crop_label(slug).to_lowercase())
                .collect(),
        );
        choices.set_filter_func({
            // A strong `entry` here would close a reference cycle with the search handler, which
            // holds `choices`; the dialog would then leak on every open.
            let entry = entry.downgrade();
            move |child| {
                let Some(entry) = entry.upgrade() else {
                    return true;
                };
                let needle = entry.text().trim().to_lowercase();
                if needle.is_empty() {
                    return true;
                }
                searchable
                    .get(child.index() as usize)
                    .map(|label| label.contains(&needle))
                    .unwrap_or(true)
            }
        });
        entry.connect_search_changed({
            let choices = choices.clone();
            move |_| choices.invalidate_filter()
        });
        search = Some(entry);

        // The full crop list is rasterized in small chunks after the dialog opens.
        load_crops(pending, loading);
    }

    let clear = gtk::Button::with_label("Use photo or default");
    clear.add_css_class("flat");
    clear.set_halign(gtk::Align::Center);
    clear.set_margin_top(10);
    clear.set_margin_bottom(10);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    if let Some(search) = &search {
        content.append(search);
    }
    content.append(&scroller(&choices));
    content.append(&clear);

    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&adw::WindowTitle::new("Choose an illustration", "")));
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&content));
    overlay.set_child(Some(&toolbar));
    dialog.set_child(Some(&overlay));

    clear.connect_clicked({
        let store = Rc::clone(store);
        let refresh = refresh.clone();
        let dialog = dialog.downgrade();
        let overlay = overlay.downgrade();
        move |_| {
            if let Err(error) = store.set_avatar_icon(owner_type, owner_id, None) {
                eprintln!("could not clear the avatar icon: {error}");
                if let Some(overlay) = overlay.upgrade() {
                    overlay.add_toast(adw::Toast::new("Could not save the avatar"));
                }
                return;
            }
            refresh();
            if let Some(dialog) = dialog.upgrade() {
                dialog.close();
            }
        }
    });

    dialog.present(Some(&window));
}

fn connect_choice(choice: &gtk::Button, key: String, save: Rc<dyn Fn(String)>) {
    choice.connect_clicked(move |_| save(key.clone()));
}

fn load_crops(pending: Rc<RefCell<VecDeque<(&'static str, gtk::Box)>>>, loading: Rc<Cell<bool>>) {
    gtk::glib::idle_add_local_once(move || {
        if !loading.get() {
            return;
        }
        let mut loaded = 0;
        loop {
            let entry = pending.borrow_mut().pop_front();
            let Some((slug, holder)) = entry else {
                return;
            };
            while let Some(child) = holder.first_child() {
                holder.remove(&child);
            }
            holder.append(&picture_widget(Picture::Crop(slug), 48));
            loaded += 1;
            if loaded >= 6 {
                break;
            }
        }
        load_crops(pending, loading);
    });
}

fn picker_flow(columns: u32) -> gtk::FlowBox {
    gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(false)
        .max_children_per_line(columns)
        .min_children_per_line(1)
        .row_spacing(10)
        .column_spacing(10)
        .margin_top(6)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .valign(gtk::Align::Start)
        .build()
}

fn choice_button(icon: impl IsA<gtk::Widget>, label: &str) -> gtk::Button {
    let (choice, preview) = choice_shell(label);
    preview.append(&icon);
    choice
}

/// The button and its empty preview area. The crop loader fills the preview when it gets there.
fn choice_shell(label: &str) -> (gtk::Button, gtk::Box) {
    let preview = gtk::Box::new(gtk::Orientation::Vertical, 0);
    preview.set_size_request(48, 48);
    preview.set_halign(gtk::Align::Center);
    preview.set_valign(gtk::Align::Center);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
    content.set_margin_top(8);
    content.set_margin_bottom(8);
    content.append(&preview);
    let text = gtk::Label::new(Some(label));
    text.add_css_class("dim-label");
    text.set_ellipsize(gtk::pango::EllipsizeMode::End);
    text.set_max_width_chars(10);
    text.set_wrap(true);
    text.set_lines(2);
    content.append(&text);

    let choice = gtk::Button::new();
    choice.add_css_class("flat");
    choice.set_tooltip_text(Some(label));
    choice.set_size_request(100, -1);
    choice.set_child(Some(&content));
    (choice, preview)
}

fn scroller(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(child)
        .build()
}

pub(in crate::ui) fn choose_photo(
    trigger: &impl IsA<gtk::Widget>,
    store: &Rc<Store>,
    owner_type: &'static str,
    owner_id: i64,
    overlay: &adw::ToastOverlay,
    refresh: Rc<dyn Fn()>,
) {
    let Some(window) = trigger
        .clone()
        .upcast::<gtk::Widget>()
        .root()
        .and_then(|root| root.downcast::<gtk::Window>().ok())
    else {
        return;
    };

    let dialog = gtk::FileDialog::builder().title("Choose a photo").build();

    let filter = gtk::FileFilter::new();
    filter.set_name(Some("Images"));
    for mime in ["image/jpeg", "image/png", "image/webp"] {
        filter.add_mime_type(mime);
    }
    let filters = gtk::gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    dialog.set_filters(Some(&filters));
    dialog.set_default_filter(Some(&filter));

    let store = store.clone();
    let overlay = overlay.clone();
    dialog.open(
        Some(&window),
        None::<&gtk::gio::Cancellable>,
        move |result| {
            let Ok(file) = result else {
                return;
            };
            let Some(path) = file.path() else {
                overlay.add_toast(adw::Toast::new("Could not open that photo"));
                return;
            };

            let role = photo_role(owner_type);
            let stem = store
                .media(owner_type, owner_id, role)
                .ok()
                .flatten()
                .map(|media| media.file_stem)
                .unwrap_or_else(|| media::stem_for(owner_type, owner_id, role));

            let imported = match media::import(&path, &stem) {
                Ok(imported) => imported,
                Err(message) => {
                    eprintln!("could not import the photo: {message}");
                    overlay.add_toast(adw::Toast::new("Could not save that photo"));
                    return;
                }
            };

            let media = Media {
                id: 0,
                owner_type: owner_type.to_string(),
                owner_id,
                role: role.to_string(),
                file_stem: stem,
                width: imported.width,
                height: imported.height,
                bytes: imported.bytes,
            };

            if let Err(error) = store.set_media(&media) {
                eprintln!("could not store the photo: {error}");
                overlay.add_toast(adw::Toast::new(
                    "Photo saved, but its details could not be stored",
                ));
                return;
            }
            if let Err(error) = store.set_avatar_icon(owner_type, owner_id, None) {
                eprintln!("could not clear the avatar icon: {error}");
            }

            refresh();
            overlay.add_toast(adw::Toast::new("Photo saved"));
        },
    );
}

fn remove_photo(
    trigger: &impl IsA<gtk::Widget>,
    store: &Rc<Store>,
    owner_type: &'static str,
    owner_id: i64,
    overlay: &adw::ToastOverlay,
    refresh: Rc<dyn Fn()>,
) {
    let store = store.clone();
    let overlay = overlay.clone();
    let role = photo_role(owner_type);

    super::form::confirm_delete(
        trigger,
        "Remove this photo?",
        "The photo is deleted from OmaGrow. This cannot be undone.",
        Rc::new(move || {
            let stem = store
                .media(owner_type, owner_id, role)
                .ok()
                .flatten()
                .map(|media| media.file_stem);

            if let Err(error) = store.delete_media(owner_type, owner_id, role) {
                eprintln!("could not remove the avatar photo: {error}");
                overlay.add_toast(adw::Toast::new("Could not remove the photo"));
                return;
            }

            if let Some(stem) = stem {
                media::remove(&stem);
            }
            refresh();
            overlay.add_toast(adw::Toast::new("Photo removed"));
        }),
    );
}

fn menu_button(label: &str) -> gtk::Button {
    let button = gtk::Button::with_label(label);
    button.add_css_class("flat");
    button.set_halign(gtk::Align::Fill);
    button
}

pub(in crate::ui) fn photo_path(stem: &str) -> Option<std::path::PathBuf> {
    [media::thumb_path(stem), media::large_path(stem)]
        .into_iter()
        .flatten()
        .find(|path| path.exists())
}

fn icon_widget(name: &str, size: i32) -> gtk::Image {
    let icon = gtk::Image::from_icon_name(name);
    icon.set_pixel_size(size);
    icon
}

pub(in crate::ui) fn picture_widget(picture: Picture, size: i32) -> gtk::Widget {
    // Rasterize at 2x and show at the logical size; this keeps the picture crisp on HiDPI screens.
    // If one asset is missing from a broken bundle, another bundled picture still beats a glyph;
    // the symbolic icon is only the last resort when even the generic plant cannot load.
    let pixbuf = match picture_pixbuf(picture, size * 2) {
        Some(pixbuf) => Some(pixbuf),
        None if picture == Picture::Crop("generic-plant") => None,
        None => picture_pixbuf(Picture::Crop("generic-plant"), size * 2),
    };
    let Some(pixbuf) = pixbuf else {
        let icon = icon_widget("omagrow-plant-symbolic", (size / 2).max(1));
        icon.set_halign(gtk::Align::Center);
        icon.set_valign(gtk::Align::Center);
        return icon.upcast();
    };

    // A GtkImage keeps the rasterized picture at its own size. A GtkPicture asks for the full
    // available width in its height-for-width measure, which turned a square hero into a square
    // page block.
    let texture = gtk::gdk::Texture::for_pixbuf(&pixbuf);
    let image = gtk::Image::from_paintable(Some(&texture));
    image.set_pixel_size(size);
    image.set_size_request(size, size);
    image.upcast()
}

fn picture_pixbuf(picture: Picture, size: i32) -> Option<Pixbuf> {
    let bytes = gtk::gio::resources_lookup_data(
        &picture_resource(picture),
        gtk::gio::ResourceLookupFlags::NONE,
    )
    .ok()?;
    let stream = gtk::gio::MemoryInputStream::from_bytes(&bytes);
    Pixbuf::from_stream_at_scale(&stream, size, size, true, None::<&gtk::gio::Cancellable>).ok()
}

fn picture_resource(picture: Picture) -> String {
    match picture {
        Picture::Crop(slug) => format!("/io/github/nicomdev/OmaGrow/crops/{slug}.svg"),
        Picture::Space(slug) => format!("/io/github/nicomdev/OmaGrow/spaces/{slug}.svg"),
        Picture::Neutral(slug) => format!("/io/github/nicomdev/OmaGrow/neutral/{slug}.svg"),
    }
}

fn icon_exists(name: &str) -> bool {
    gtk::gdk::Display::default()
        .map(|display| gtk::IconTheme::for_display(&display).has_icon(name))
        .unwrap_or(true)
}

/// A square, centre-cropped avatar at exactly `size`. A `GtkPicture` keeps the image's natural
/// size as its minimum, so a 1600 px photo would otherwise blow a list row up to hundreds of
/// pixels. Scaling the pixbuf first keeps the row at the requested size.
fn avatar_pixbuf(path: &std::path::Path, size: i32) -> Option<Pixbuf> {
    let pixbuf = Pixbuf::from_file(path).ok()?;
    let side = pixbuf.width().min(pixbuf.height()).max(1);
    let square = if side == pixbuf.width() && side == pixbuf.height() {
        pixbuf
    } else {
        let x = (pixbuf.width() - side) / 2;
        let y = (pixbuf.height() - side) / 2;
        pixbuf.new_subpixbuf(x, y, side, side)
    };

    square.scale_simple(size, size, InterpType::Bilinear)
}

#[cfg(test)]
mod tests {
    use super::avatar_pixbuf;
    use gdk_pixbuf::{Colorspace, Pixbuf};

    #[test]
    fn every_illustration_has_a_bundled_asset() {
        gtk::gio::resources_register_include!("omagrow.gresource")
            .expect("the bundled resources register");

        for slug in super::CROPS {
            assert!(
                super::picture_pixbuf(super::Picture::Crop(slug), 40).is_some(),
                "missing crop asset for {} ({slug})",
                super::crop_label(slug)
            );
        }
        for slug in super::SPACES {
            assert!(
                super::picture_pixbuf(super::Picture::Space(slug), 40).is_some(),
                "missing space asset for {} ({slug})",
                super::space_label(slug)
            );
        }
        assert!(
            super::picture_pixbuf(super::Picture::Neutral("sprout"), 40).is_some(),
            "missing the neutral planting sprout"
        );
    }

    #[test]
    fn a_stored_key_resolves_to_its_own_picture() {
        assert_eq!(
            super::picture_for_key("crop:tomato"),
            Some(super::Picture::Crop("tomato"))
        );
        assert_eq!(
            super::picture_for_key("space:greenhouse"),
            Some(super::Picture::Space("greenhouse"))
        );
        assert_eq!(super::picture_for_key("crop:not-a-crop"), None);
        assert_eq!(super::picture_for_key("space:not-a-place"), None);
        assert_eq!(super::picture_for_key("omagrow-plant-symbolic"), None);
    }

    #[test]
    fn each_owner_falls_back_to_its_own_picture() {
        use super::{Picture, fallback_picture};

        assert_eq!(
            fallback_picture("profile", None),
            Picture::Crop("seed-packet")
        );
        assert_eq!(
            fallback_picture("planting", None),
            Picture::Neutral("sprout")
        );
        assert_eq!(
            fallback_picture("space", Some("outdoor")),
            Picture::Space("garden-bed")
        );
        assert_eq!(
            fallback_picture("space", Some("greenhouse")),
            Picture::Space("greenhouse")
        );
        assert_eq!(
            fallback_picture("space", Some("indoor")),
            Picture::Space("indoor-shelf")
        );
        assert_eq!(
            fallback_picture("space", None),
            Picture::Space("garden-bed")
        );
    }

    #[test]
    fn the_owner_decides_the_fallback_and_stored_keys_win_over_it() {
        use super::{Picture, Source, source};
        use crate::planting::SpaceInput;
        use crate::store::Store;

        let store = Store::open_in_memory().unwrap();
        let profile = store.create_profile("Basil", None, None).unwrap();
        assert_eq!(
            source(&store, "profile", profile),
            Source::Default(Picture::Crop("seed-packet"))
        );
        store
            .set_avatar_icon("profile", profile, Some("crop:tomato"))
            .unwrap();
        assert_eq!(
            source(&store, "profile", profile),
            Source::Picture(Picture::Crop("tomato"))
        );
        store
            .set_avatar_icon("profile", profile, Some("crop:not-a-crop"))
            .unwrap();
        assert_eq!(
            source(&store, "profile", profile),
            Source::Default(Picture::Crop("seed-packet"))
        );

        let space = store
            .create_space(&SpaceInput {
                name: Some("Shelf".into()),
                kind: "indoor".into(),
                light_type: Some("led".into()),
                light_hours: Some(12.0),
                light_start: Some(6 * 60),
                notes: None,
            })
            .unwrap();
        assert_eq!(
            source(&store, "space", space),
            Source::Default(Picture::Space("indoor-shelf"))
        );
        store
            .set_avatar_icon("space", space, Some("space:grow-tent"))
            .unwrap();
        assert_eq!(
            source(&store, "space", space),
            Source::Picture(Picture::Space("grow-tent"))
        );
    }

    #[test]
    fn an_avatar_is_a_square_at_the_requested_size() {
        let directory = std::env::temp_dir().join(format!("omagrow-avatar-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&directory);
        let path = directory.join("photo.png");
        let source = Pixbuf::new(Colorspace::Rgb, false, 8, 400, 200).expect("a pixbuf");
        source.fill(0xcc3311ff);
        source.savev(&path, "png", &[]).unwrap();

        let avatar = avatar_pixbuf(&path, 56).expect("an avatar");
        assert_eq!((avatar.width(), avatar.height()), (56, 56));

        let _ = std::fs::remove_dir_all(&directory);
    }
}
