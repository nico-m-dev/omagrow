//! The Help page: a short, visual overview of what OmaGrow does.
//!
//! Each section puts its text beside a screenshot on a wide page and stacks them on a narrow one.
//! [`Pair`], a small layout manager, makes that choice from the width it is given. A `FlowBox`
//! cannot: it sizes its children from their natural width, and a wrapping label's natural width
//! changes with the width it gets, so the sections jumped and the screenshots stretched.

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use libadwaita as adw;

use super::page_header;

/// The gap between the text and the screenshot, and between two stacked parts.
const GAP: i32 = 32;
/// The screenshot width in both layouts.
const SHOT_WIDTH: i32 = 400;
/// Below this content width the screenshot moves under the text. It leaves the text at least
/// 328 px beside a 400 px screenshot.
const SIDE_BY_SIDE_FROM: i32 = 760;

pub(super) fn page() -> gtk::Widget {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 40);
    content.set_margin_top(32);
    content.set_margin_bottom(40);
    content.set_margin_start(24);
    content.set_margin_end(24);

    content.append(&hero());

    for feature in [
        Feature {
            image: "profiles.png",
            alt: "Day and night temperature ranges, with the growing cycle below them.",
            icon: "omagrow-plant-symbolic",
            eyebrow: "PLANT PROFILES",
            title: "Know what your plants need.",
            body: "Stages, temperature ranges, timing and traits, with a source on every value. \
                   You confirm what is right; the rest stays an honest estimate.",
            chips: &[],
            shot_first: true,
        },
        Feature {
            image: "plantings.png",
            alt: "A planting calendar for November, with one day selected and its entry below.",
            icon: "omagrow-planting-symbolic",
            eyebrow: "PLANTINGS",
            title: "Remember what actually happened.",
            body: "A timeline, a calendar, a diary and climate graphs from the space's readings. \
                   Pick a day and see the whole story: what you did, how the plant responded and \
                   what the weather did outdoors.",
            chips: &[],
            shot_first: false,
        },
        Feature {
            image: "spaces.png",
            alt: "An indoor space with shared light, its plantings and its readings taken by hand.",
            icon: "omagrow-space-symbolic",
            eyebrow: "SPACES",
            title: "See the climate they share.",
            body: "Group plantings by the place they grow. Readings and day/night follow the \
                   space; outdoors it also gets the local weather.",
            chips: &[],
            shot_first: true,
        },
        Feature {
            image: "agent.png",
            alt: "A profile's activity list: each recorded change with its source and a Revert \
                  button.",
            icon: "preferences-system-symbolic",
            eyebrow: "YOUR AGENT",
            title: "An extra pair of eyes. On your terms.",
            body: "Your own agent reads and writes through the app's validation. Sources stay \
                   visible, confirmed values stay untouched, and every recorded change can be \
                   reverted until a newer value replaces it. Your grow stays local: no account, no \
                   cloud, no telemetry. Open-Meteo is the only outside call. Ask your agent to run \
                   “omagrow agent guide” once, so it knows the rules.",
            chips: &["Local SQLite", "Sourced values", "Revert recorded changes"],
            shot_first: false,
        },
    ] {
        content.append(&feature.section());
    }

    content.append(&credits());

    let clamp = adw::Clamp::new();
    clamp.set_maximum_size(960);
    clamp.set_tightening_threshold(1100);
    clamp.set_child(Some(&content));

    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&clamp)
        .build();
    // The page itself has little to focus, so give the scroller the focus for Page Down and the
    // arrow keys. It does not grab it: the sidebar keeps the focus when the page opens.
    scroller.set_focusable(true);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&page_header("Help"));
    toolbar.set_content(Some(&scroller));
    toolbar.upcast()
}

fn hero() -> gtk::Widget {
    let text = gtk::Box::new(gtk::Orientation::Vertical, 10);
    text.append(&label("OMAGROW", "omagrow-help-eyebrow"));
    text.append(&label("A grow with a memory.", "omagrow-help-hero-title"));
    text.append(&label(
        "Profiles that know what a plant needs. Plantings that remember what happened. Spaces \
         that share the weather, the light and the air.",
        "omagrow-help-lead",
    ));

    pair(
        text,
        shot(
            "hero.png",
            "A list of plantings, each with its space, stage and day count.",
        ),
        false,
    )
}

/// One feature section: a short text and a screenshot, side by side on a wide page.
struct Feature<'a> {
    image: &'a str,
    alt: &'a str,
    icon: &'a str,
    eyebrow: &'a str,
    title: &'a str,
    body: &'a str,
    chips: &'a [&'a str],
    shot_first: bool,
}

impl Feature<'_> {
    fn section(&self) -> gtk::Widget {
        let text = gtk::Box::new(gtk::Orientation::Vertical, 10);

        let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        head.set_valign(gtk::Align::Center);

        let badge = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        badge.add_css_class("omagrow-help-badge");
        badge.set_valign(gtk::Align::Center);
        let badge_image = gtk::Image::from_icon_name(self.icon);
        badge_image.set_pixel_size(14);
        badge.append(&badge_image);
        head.append(&badge);

        let eyebrow = label(self.eyebrow, "omagrow-help-eyebrow");
        eyebrow.set_wrap(false);
        head.append(&eyebrow);
        text.append(&head);

        text.append(&label(self.title, "omagrow-help-title"));
        text.append(&label(self.body, "omagrow-help-body"));

        if !self.chips.is_empty() {
            let rows = gtk::Box::new(gtk::Orientation::Vertical, 6);
            rows.set_halign(gtk::Align::Start);
            for pair in self.chips.chunks(2) {
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                row.set_halign(gtk::Align::Start);
                for chip in pair {
                    row.append(&label(chip, "omagrow-help-chip"));
                }
                rows.append(&row);
            }
            text.append(&rows);
        }

        pair(text, shot(self.image, self.alt), self.shot_first)
    }
}

fn pair(text: gtk::Box, image: gtk::Widget, shot_first: bool) -> gtk::Widget {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.append(&text);
    root.append(&image);

    let layout = Pair::new(shot_first);
    root.set_layout_manager(Some(layout));
    root.upcast()
}

fn shot(resource: &str, alt: &str) -> gtk::Widget {
    let path = format!("/io/github/nicomdev/OmaGrow/help/{resource}");
    // The source stays at its own size; GTK scales the texture to the card. Pre-scaling it to the
    // display size made the screenshot soft again on a HiDPI screen.
    let texture = gtk::gdk::Texture::from_resource(&path);

    let picture = gtk::Picture::for_paintable(&texture);
    picture.set_alternative_text(Some(alt));
    picture.set_content_fit(gtk::ContentFit::Cover);
    picture.set_can_shrink(true);
    picture.set_hexpand(true);
    picture.set_vexpand(true);

    let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
    frame.add_css_class("omagrow-help-shot");
    frame.set_overflow(gtk::Overflow::Hidden);
    frame.append(&picture);
    frame.upcast()
}

fn credits() -> gtk::Widget {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 18);
    card.add_css_class("omagrow-help-credits");

    let heading = label("Credits", "omagrow-help-credit-heading");
    card.append(&heading);

    card.append(&credit_group(
        "Crop illustrations",
        "The crop avatars come from Open Crop Icons by the OpenFarm community, released under \
         CC0. The seed packet and the twelve space illustrations are drawn for OmaGrow.",
        &[(
            "https://github.com/openfarmcc/open-crop-icons",
            "Open Crop Icons on GitHub",
        )],
    ));

    card.append(&credit_group(
        "Weather and places",
        "Weather, the frost estimate and the place search come from Open-Meteo, CC BY 4.0. Place \
         names come from GeoNames. The frost estimate uses the ERA5-Land reanalysis for 1991–2020.",
        &[
            ("https://open-meteo.com", "Open-Meteo"),
            ("https://www.geonames.org", "GeoNames"),
        ],
    ));

    card.upcast()
}

fn credit_group(title: &str, text: &str, links: &[(&str, &str)]) -> gtk::Widget {
    let group = gtk::Box::new(gtk::Orientation::Vertical, 6);

    group.append(&label(title, "omagrow-help-credit-title"));

    let body = label(text, "dim-label");
    body.set_max_width_chars(78);
    group.append(&body);

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    row.set_halign(gtk::Align::Start);
    for (url, text) in links {
        let link = gtk::LinkButton::with_label(url, text);
        link.set_halign(gtk::Align::Start);
        row.append(&link);
    }
    group.append(&row);

    group.upcast()
}

fn label(text: &str, class: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_max_width_chars(46);
    label.add_css_class(class);
    label
}

mod pair_layout {
    use gtk::glib;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;

    #[derive(Default)]
    pub struct PairLayout {
        /// Put the screenshot in the first column, before the text.
        pub(super) shot_first: std::cell::Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PairLayout {
        const NAME: &'static str = "OmaGrowHelpPair";
        type Type = super::Pair;
        type ParentType = gtk::LayoutManager;
    }

    impl ObjectImpl for PairLayout {}

    impl LayoutManagerImpl for PairLayout {
        fn request_mode(&self, _widget: &gtk::Widget) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(
            &self,
            widget: &gtk::Widget,
            orientation: gtk::Orientation,
            for_size: i32,
        ) -> (i32, i32, i32, i32) {
            let Some(text) = widget.first_child() else {
                return (0, 0, -1, -1);
            };
            if text.next_sibling().is_none() {
                return (0, 0, -1, -1);
            }

            if orientation == gtk::Orientation::Horizontal {
                // The page clamp and the scroller decide the width; only the text needs a real
                // minimum so the window never has to be wider than its longest word.
                let (minimum, _, _, _) = text.measure(orientation, for_size);
                return (minimum, super::SIDE_BY_SIDE_FROM, -1, -1);
            }

            // GTK passes -1 when the width is unknown: the horizontal measure above reports the
            // width at which the parts sit side by side, so measure there instead of at 0.
            let width = if for_size < 0 {
                super::SIDE_BY_SIDE_FROM
            } else {
                for_size
            };
            let parts = super::parts(width);
            let (_, text_height, _, _) = text.measure(gtk::Orientation::Vertical, parts.text_width);
            let height = if parts.side_by_side {
                text_height.max(parts.image_height)
            } else {
                text_height + super::GAP + parts.image_height
            };
            (height, height, -1, -1)
        }

        fn allocate(&self, widget: &gtk::Widget, width: i32, height: i32, baseline: i32) {
            let Some(text) = widget.first_child() else {
                return;
            };
            let Some(image) = text.next_sibling() else {
                return;
            };

            let parts = super::parts(width.max(0));
            let shot_first = self.shot_first.get();
            let (_, text_height, _, _) = text.measure(gtk::Orientation::Vertical, parts.text_width);

            if parts.side_by_side {
                let text_x = if shot_first {
                    parts.image_width + super::GAP
                } else {
                    0
                };
                let image_x = if shot_first {
                    0
                } else {
                    parts.text_width + super::GAP
                };
                let text_shift = gtk::gsk::Transform::new().translate(&gtk::graphene::Point::new(
                    text_x as f32,
                    ((height - text_height) / 2).max(0) as f32,
                ));
                let image_shift = gtk::gsk::Transform::new()
                    .translate(&gtk::graphene::Point::new(image_x as f32, 0.0));
                text.allocate(parts.text_width, text_height, baseline, Some(text_shift));
                image.allocate(
                    parts.image_width,
                    parts.image_height.min(height),
                    -1,
                    Some(image_shift),
                );
            } else {
                // The screenshot starts at the text's left edge, so a stacked section reads as one
                // column instead of a centred picture under a left-aligned text.
                let image_shift = gtk::gsk::Transform::new().translate(&gtk::graphene::Point::new(
                    0.0,
                    (text_height + super::GAP) as f32,
                ));
                text.allocate(width, text_height, baseline, None);
                image.allocate(
                    parts.image_width,
                    parts
                        .image_height
                        .min((height - text_height - super::GAP).max(0)),
                    -1,
                    Some(image_shift),
                );
            }
        }
    }
}

glib::wrapper! {
    pub struct Pair(ObjectSubclass<pair_layout::PairLayout>)
        @extends gtk::LayoutManager;
}

impl Pair {
    fn new(shot_first: bool) -> Self {
        let pair: Self = glib::Object::builder().build();
        pair.imp().shot_first.set(shot_first);
        pair
    }
}

/// Where the two parts go at a given content width.
struct Parts {
    side_by_side: bool,
    text_width: i32,
    image_width: i32,
    image_height: i32,
}

fn parts(width: i32) -> Parts {
    let width = width.max(0);
    let side_by_side = width >= SIDE_BY_SIDE_FROM;
    // The screenshot keeps one size in both layouts. On a narrow page it sits under the text at
    // the width a screenshot can actually be sharp at, instead of being stretched to the page.
    let image_width = SHOT_WIDTH.min(width);
    let text_width = if side_by_side {
        (width - image_width - GAP).max(0)
    } else {
        width
    };
    Parts {
        side_by_side,
        text_width,
        image_width,
        image_height: image_height(image_width),
    }
}

fn image_height(width: i32) -> i32 {
    if width <= 0 {
        return 0;
    }
    (width * 2 / 3).max(1)
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_help_image_is_bundled_and_three_by_two() {
        gtk::gio::resources_register_include!("omagrow.gresource")
            .expect("the bundled resources register");

        for name in [
            "hero.png",
            "profiles.png",
            "plantings.png",
            "spaces.png",
            "agent.png",
        ] {
            let path = format!("/io/github/nicomdev/OmaGrow/help/{name}");
            let bytes = gtk::gio::resources_lookup_data(&path, gtk::gio::ResourceLookupFlags::NONE)
                .unwrap_or_else(|_| panic!("missing Help image {name}"));

            let stream = gtk::gio::MemoryInputStream::from_bytes(&bytes);
            let pixbuf = gdk_pixbuf::Pixbuf::from_stream(&stream, None::<&gtk::gio::Cancellable>)
                .expect("the Help image decodes");
            // The cards and the layout are 3:2; another ratio would be cropped by ContentFit::Cover.
            let (width, height) = (pixbuf.width(), pixbuf.height());
            assert!(
                (width * 2 - height * 3).abs() <= 2,
                "{name} is {width}×{height}, not 3:2"
            );
        }
    }

    #[test]
    fn the_pair_stacks_below_its_breakpoint() {
        let wide = super::parts(super::SIDE_BY_SIDE_FROM);
        assert!(wide.side_by_side);
        assert_eq!(wide.image_width, super::SHOT_WIDTH);
        assert_eq!(
            wide.text_width,
            super::SIDE_BY_SIDE_FROM - super::SHOT_WIDTH - super::GAP
        );

        let narrow = super::parts(super::SIDE_BY_SIDE_FROM - 1);
        assert!(!narrow.side_by_side);
        assert_eq!(narrow.text_width, super::SIDE_BY_SIDE_FROM - 1);
        assert_eq!(narrow.image_width, super::SHOT_WIDTH);

        let tiny = super::parts(320);
        assert_eq!(tiny.image_width, 320);
        assert_eq!(tiny.image_height, 213);
    }
}
