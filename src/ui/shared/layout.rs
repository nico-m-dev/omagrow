//! A page body that becomes two columns when the window is wide enough.
//!
//! The narrow layout is the one column OmaGrow has always used: a reading column of 672 px (the
//! old 720 px clamp minus its 24 px margins). From a 1426 px window the body splits into a main
//! column and a rail of 400 px; it stays split down to 1386 px, where the main column is still
//! 672 px wide, and turns back into one column below that. The 40 px band keeps a window that is
//! dragged over the edge from flickering. Cards keep their declared order when the layout is
//! narrow, and are grouped by slot when it is wide, so a page does not need two builds.
//!
//! The split has its own layout manager instead of a box with two children. A box brings every
//! child up to its natural width before it gives the rest to the expanding one, and a wrapping
//! label's natural width depends on the text of the selected day; that made the rail grow and the
//! main column shrink whenever another day was picked. The layout manager fixes both widths.
//!
//! GTK4 has no signal for a widget's own width, and the body cannot use its own width anyway: in
//! the two-column layout its minimum width keeps the body from shrinking when the window does.
//! The window width is the honest measure, so the body listens to the window surface and decides
//! from there.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use libadwaita as adw;

/// Where a card goes when the body has two columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum Slot {
    Main,
    Side,
    /// Across the whole content width, below the two columns. On a narrow page it stays in the
    /// one-column order, so the push order is the reading order.
    Full,
}

/// The page clamp of the two-column pages: the old 720 px reading column, and the wider body.
const NARROW_WIDTH: i32 = 720;
const WIDE_WIDTH: i32 = 1200;
/// The rail, and the gap between the two columns.
const RAIL_WIDTH: i32 = 400;
const GAP: i32 = 18;
/// Window width at which the body starts wide. The split needs 1386 px (248 px sidebar + 48 px
/// margins + 672 px main + 18 px gap + 400 px rail); the extra 40 px keeps the main column at
/// least 672 px when the window is dragged smaller again.
const WIDE_FROM: i32 = 1426;
/// Below this the body returns to one column. At this width the main column is exactly 672 px.
const NARROW_BELOW: i32 = 1386;

thread_local! {
    /// The bodies that are on screen or on their way there. Weak, so a closed page leaves nothing
    /// behind.
    static WATCHERS: RefCell<Vec<Weak<Inner>>> = const { RefCell::new(Vec::new()) };
    /// The window surfaces whose size changes are watched, installed once per window.
    static SURFACES: RefCell<Vec<glib::WeakRef<gtk::gdk::Surface>>> =
        const { RefCell::new(Vec::new()) };
}

mod split {
    use super::{GAP, RAIL_WIDTH};
    use gtk::glib;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;

    #[derive(Default)]
    pub struct SplitLayout {
        pub(super) wide: std::cell::Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SplitLayout {
        const NAME: &'static str = "OmaGrowSplitLayout";
        type Type = super::SplitLayout;
        type ParentType = gtk::LayoutManager;
    }

    impl ObjectImpl for SplitLayout {}

    impl LayoutManagerImpl for SplitLayout {
        fn request_mode(&self, _widget: &gtk::Widget) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(
            &self,
            widget: &gtk::Widget,
            orientation: gtk::Orientation,
            for_size: i32,
        ) -> (i32, i32, i32, i32) {
            let Some(main) = widget.first_child() else {
                return (0, 0, -1, -1);
            };
            let side = main.next_sibling();
            let full = side.as_ref().and_then(|side| side.next_sibling());
            let wide = self.wide.get() && side.as_ref().is_some_and(|side| side.is_visible());
            let full_visible = full.as_ref().is_some_and(|full| full.is_visible());

            if orientation == gtk::Orientation::Horizontal {
                let (minimum, natural, min_baseline, nat_baseline) =
                    main.measure(orientation, for_size);
                if !wide {
                    return (minimum, natural, min_baseline, nat_baseline);
                }

                let (side_minimum, side_natural, _, _) =
                    side.unwrap().measure(orientation, for_size);
                // The rail is fixed; only the main column follows the window. A side minimum
                // above the rail would clip, and no rail card has one.
                let mut minimum = minimum + GAP + side_minimum.max(RAIL_WIDTH);
                let mut natural = natural + GAP + side_natural.max(RAIL_WIDTH);
                if let Some(full) = full.filter(|_| full_visible) {
                    let (full_minimum, full_natural, _, _) = full.measure(orientation, for_size);
                    minimum = minimum.max(full_minimum);
                    natural = natural.max(full_natural);
                }
                return (minimum, natural, min_baseline, nat_baseline);
            }

            let main_width = if wide {
                (for_size - GAP - RAIL_WIDTH).max(0)
            } else {
                for_size
            };
            let (_, main_natural, min_baseline, nat_baseline) =
                main.measure(orientation, main_width);
            let mut height = main_natural;
            if wide {
                let (_, side_natural, _, _) = side.unwrap().measure(orientation, RAIL_WIDTH);
                height = height.max(side_natural);
            }
            if let Some(full) = full.filter(|_| full_visible) {
                let (_, full_natural, _, _) = full.measure(orientation, for_size);
                height += GAP + full_natural;
            }
            // The body is content in a scroller. A parent that allocates the minimum height would
            // clip the bottom of a card or make a lower row unclickable, so the page's minimum is
            // its natural height; the window can still be smaller because the scroller handles it.
            (height, height, min_baseline, nat_baseline)
        }

        fn allocate(&self, widget: &gtk::Widget, width: i32, height: i32, baseline: i32) {
            let Some(main) = widget.first_child() else {
                return;
            };
            let side = main.next_sibling();
            let full = side.as_ref().and_then(|side| side.next_sibling());
            let wide = self.wide.get() && side.as_ref().is_some_and(|side| side.is_visible());
            let main_width = if wide {
                (width - GAP - RAIL_WIDTH).max(0)
            } else {
                width
            };

            let (_, main_height, _, _) = main.measure(gtk::Orientation::Vertical, main_width);
            let mut first_height = main_height;
            if wide {
                let (_, side_height, _, _) = side
                    .as_ref()
                    .unwrap()
                    .measure(gtk::Orientation::Vertical, RAIL_WIDTH);
                first_height = first_height.max(side_height);
            }
            let full_top = if full.as_ref().is_some_and(|full| full.is_visible()) {
                first_height + GAP
            } else {
                height
            };

            main.allocate(main_width, main_height.min(height), baseline, None);

            if let Some(side) = side.filter(|_| wide) {
                let (_, side_height, _, _) = side.measure(gtk::Orientation::Vertical, RAIL_WIDTH);
                let shift = gtk::gsk::Transform::new()
                    .translate(&gtk::graphene::Point::new((main_width + GAP) as f32, 0.0));
                side.allocate(RAIL_WIDTH, side_height.min(height), baseline, Some(shift));
            }

            if let Some(full) = full.filter(|full| full.is_visible()) {
                let remaining = (height - full_top).max(0);
                let (_, full_height, _, _) = full.measure(gtk::Orientation::Vertical, width);
                let shift = gtk::gsk::Transform::new()
                    .translate(&gtk::graphene::Point::new(0.0, full_top as f32));
                full.allocate(width, full_height.min(remaining), baseline, Some(shift));
            }
        }
    }
}

glib::wrapper! {
    pub struct SplitLayout(ObjectSubclass<split::SplitLayout>)
        @extends gtk::LayoutManager;
}

impl SplitLayout {
    fn new() -> Self {
        glib::Object::builder().build()
    }

    fn set_wide(&self, wide: bool) {
        self.imp().wide.set(wide);
    }
}

/// A page body with a main column and a rail.
pub(in crate::ui) struct Columns {
    inner: Rc<Inner>,
}

struct Inner {
    root: gtk::Box,
    main: gtk::Box,
    side: gtk::Box,
    full: gtk::Box,
    layout: SplitLayout,
    /// The page's clamp, and the width it gets in each mode.
    clamp: adw::Clamp,
    narrow_max: i32,
    wide_max: i32,
    /// Whether this page can split into a rail. A list widens without one.
    rail: bool,
    entries: RefCell<Vec<(gtk::Widget, Slot)>>,
    wide: Cell<bool>,
    pending: Cell<bool>,
}

impl Columns {
    /// A detail page: one 720 px column, or a main column and a 400 px rail.
    pub(in crate::ui) fn new(clamp: &adw::Clamp) -> Self {
        Self::build(clamp, NARROW_WIDTH, WIDE_WIDTH, true)
    }

    /// A list page: one column that widens on a large window, without a rail.
    pub(in crate::ui) fn widening(clamp: &adw::Clamp, narrow_max: i32, wide_max: i32) -> Self {
        Self::build(clamp, narrow_max, wide_max, false)
    }

    fn build(clamp: &adw::Clamp, narrow_max: i32, wide_max: i32, rail: bool) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let main = gtk::Box::new(gtk::Orientation::Vertical, GAP);
        main.set_valign(gtk::Align::Start);
        let side = gtk::Box::new(gtk::Orientation::Vertical, GAP);
        side.set_valign(gtk::Align::Start);
        side.set_visible(false);
        let full = gtk::Box::new(gtk::Orientation::Vertical, GAP);
        full.set_valign(gtk::Align::Start);
        full.set_visible(false);
        root.append(&main);
        root.append(&side);
        root.append(&full);

        let layout = SplitLayout::new();
        root.set_layout_manager(Some(layout.clone()));

        // A tightening threshold below the maximum eases the width out and keeps the child below
        // its maximum until the window is much wider; this layout wants the full width at once.
        clamp.set_tightening_threshold(WIDE_WIDTH);

        let inner = Rc::new(Inner {
            root,
            main,
            side,
            full,
            layout,
            clamp: clamp.clone(),
            narrow_max,
            wide_max,
            rail,
            entries: RefCell::new(Vec::new()),
            wide: Cell::new(false),
            pending: Cell::new(false),
        });

        inner.watch();

        Self { inner }
    }

    pub(in crate::ui) fn widget(&self) -> &gtk::Widget {
        self.inner.root.upcast_ref()
    }

    /// A card in both layouts, on one side of the split.
    pub(in crate::ui) fn push(&self, widget: &impl IsA<gtk::Widget>, slot: Slot) {
        self.inner.add(widget, slot);
    }

    /// Place the cards once the page is built, starting in the mode the window is in. Until this
    /// runs they sit in the main column, which is the narrow order and is never shown before the
    /// first allocation.
    pub(in crate::ui) fn finish(&self, window_width: i32) {
        let wide = window_width >= WIDE_FROM && self.inner.can_be_wide();
        self.inner.wide.set(wide);
        self.inner.apply_mode();
    }
}

impl Inner {
    fn watch(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.root.connect_map(move |_| {
            if let Some(inner) = weak.upgrade() {
                inner.register();
                queue_check(&inner);
            }
        });
    }

    /// Put this body on the list that a window resize wakes up, and install the one surface
    /// handler the window needs.
    fn register(self: &Rc<Self>) {
        WATCHERS.with(|watchers| {
            let mut watchers = watchers.borrow_mut();
            watchers.retain(|weak| weak.strong_count() > 0);
            let known = watchers
                .iter()
                .any(|weak| std::ptr::eq(weak.as_ptr(), Rc::as_ptr(self)));
            if !known {
                watchers.push(Rc::downgrade(self));
            }
        });

        let Some(window) = self
            .root
            .root()
            .and_then(|root| root.downcast::<gtk::Window>().ok())
        else {
            return;
        };
        let Some(surface) = window.surface() else {
            return;
        };

        let known = SURFACES.with(|surfaces| {
            let mut surfaces = surfaces.borrow_mut();
            surfaces.retain(|weak| weak.upgrade().is_some());
            surfaces
                .iter()
                .any(|weak| weak.upgrade().is_some_and(|installed| installed == surface))
        });
        if known {
            return;
        }

        SURFACES.with(|surfaces| surfaces.borrow_mut().push(surface.downgrade()));
        surface.connect_layout(|_, _, _| {
            let inners: Vec<Rc<Inner>> = WATCHERS.with(|watchers| {
                let mut watchers = watchers.borrow_mut();
                watchers.retain(|weak| weak.strong_count() > 0);
                watchers.iter().filter_map(|weak| weak.upgrade()).collect()
            });

            for inner in inners {
                queue_check(&inner);
            }
        });
    }

    fn add(self: &Rc<Self>, widget: &impl IsA<gtk::Widget>, slot: Slot) {
        let widget = widget.clone().upcast::<gtk::Widget>();
        self.entries.borrow_mut().push((widget.clone(), slot));
        self.main.append(&widget);
    }

    fn has_side(&self) -> bool {
        self.entries
            .borrow()
            .iter()
            .any(|(_, slot)| *slot == Slot::Side)
    }

    /// A page without side cards stays one column; a list still widens.
    fn can_be_wide(&self) -> bool {
        !self.rail || self.has_side()
    }

    fn check(&self, window_width: i32) {
        if let Some(next) = mode_change(self.wide.get(), window_width, self.can_be_wide()) {
            self.wide.set(next);
            self.apply_mode();
        }
    }

    fn apply_mode(&self) {
        let wide = self.wide.get() && self.can_be_wide();
        // A list widens without a rail: the clamp grows, but the body stays one column so an
        // empty rail does not eat 400 px.
        let two_column = self.rail && wide;
        let entries: Vec<(gtk::Widget, Slot)> = self
            .entries
            .borrow()
            .iter()
            .map(|(widget, slot)| (widget.clone(), *slot))
            .collect();

        self.clamp
            .set_maximum_size(if wide { self.wide_max } else { self.narrow_max });
        self.layout.set_wide(two_column);

        // A mode change rebuilds both columns; the cards that stay in the main column are moved
        // too, because their order changes when the rail cards come back in.
        for (widget, _) in &entries {
            if widget.parent().is_some() {
                widget.unparent();
            }
        }

        for (widget, slot) in &entries {
            widget.set_visible(true);

            if two_column && *slot == Slot::Side {
                self.side.append(widget);
            } else if two_column && *slot == Slot::Full {
                self.full.append(widget);
            } else {
                self.main.append(widget);
            }
        }

        self.side.set_visible(two_column);
        self.full.set_visible(two_column && self.has_full());
    }
}

impl Inner {
    fn has_full(&self) -> bool {
        self.entries
            .borrow()
            .iter()
            .any(|(_, slot)| *slot == Slot::Full)
    }
}

/// Whether the body should be wide, given its current mode and the window width. Separate from
/// the widgets so the thresholds can be tested without a display.
fn decide(wide: bool, window_width: i32) -> bool {
    if wide {
        window_width >= NARROW_BELOW
    } else {
        window_width >= WIDE_FROM
    }
}

/// The mode to switch to, or `None` when nothing should change. A page that cannot be wide stays
/// narrow even when the window is large; without this the check would rebuild the cards on every
/// layout pass and a click on a row would be lost.
fn mode_change(wide: bool, window_width: i32, can_be_wide: bool) -> Option<bool> {
    if !can_be_wide {
        return wide.then_some(false);
    }

    let next = decide(wide, window_width);
    (next != wide).then_some(next)
}

/// The width the layout decision is made on: the whole window, not the body. A page that is built
/// before it is shown can still ask its navigation view for the window.
pub(in crate::ui) fn window_width(widget: &impl IsA<gtk::Widget>) -> i32 {
    widget
        .as_ref()
        .root()
        .and_then(|root| root.downcast::<gtk::Window>().ok())
        .map(|window| window.width())
        .unwrap_or(0)
}

/// Calls `changed` with the widget's own width when it is mapped and whenever the surface is laid
/// out. The climate section switches between one tabbed plot and a 2×2 grid on its own width, not
/// the window: a page that stays one column must keep the tabbed single plot.
pub(in crate::ui) fn watch_widget_width(widget: &impl IsA<gtk::Widget>, changed: Rc<dyn Fn(i32)>) {
    let widget = widget.clone().upcast::<gtk::Widget>();
    let installed = Rc::new(Cell::new(false));

    widget.connect_map(move |widget| {
        changed(widget.width());

        if installed.replace(true) {
            return;
        }
        let Some(window) = widget
            .root()
            .and_then(|root| root.downcast::<gtk::Window>().ok())
        else {
            return;
        };
        if let Some(surface) = window.surface() {
            let changed = changed.clone();
            let watched = widget.clone();
            let handler = Cell::new(Some(
                surface.connect_layout(move |_, _, _| changed(watched.width())),
            ));
            let surface = surface.clone();
            widget.connect_destroy(move |_| {
                if let Some(handler) = handler.take() {
                    surface.disconnect(handler);
                }
            });
        }
    });
}

/// Ask for a width check once the main loop is idle again. This used to run on the frame clock,
/// but the app has no other frame-clock user and an idle is enough for a layout switch; it keeps
/// the compositor's frame timings out of the app.
fn queue_check(inner: &Rc<Inner>) {
    if inner.pending.replace(true) {
        return;
    }

    let weak = Rc::downgrade(inner);
    glib::idle_add_local_once(move || {
        let Some(inner) = weak.upgrade() else {
            return;
        };

        inner.pending.set(false);
        let width = window_width(&inner.root);
        if width > 0 {
            inner.check(width);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{NARROW_BELOW, WIDE_FROM, decide, mode_change};

    #[test]
    fn a_narrow_body_only_splits_once_the_split_really_fits() {
        assert!(!decide(false, WIDE_FROM - 1));
        assert!(decide(false, WIDE_FROM));
    }

    #[test]
    fn a_wide_body_keeps_its_main_column_at_least_672_px() {
        assert!(decide(true, NARROW_BELOW));
        assert!(!decide(true, NARROW_BELOW - 1));
    }

    #[test]
    fn a_page_without_a_side_card_does_not_rebuild_on_a_wide_window() {
        assert_eq!(mode_change(false, WIDE_FROM, false), None);
        assert_eq!(mode_change(true, WIDE_FROM, false), Some(false));
        assert_eq!(mode_change(false, WIDE_FROM, true), Some(true));
        assert_eq!(mode_change(true, NARROW_BELOW, true), None);
        assert_eq!(mode_change(true, NARROW_BELOW - 1, true), Some(false));
    }
}
