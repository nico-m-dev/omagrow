//! The sidebar shell: the areas, the pages pushed inside them, and leaving without losing input.
//!
//! The sidebar is the top level. An area always opens at its list, and leaving an area closes the
//! pages that were pushed inside it. A form page carries a [`Dirty`] flag: as soon as the user
//! touches a field, every way out (Cancel, the back button, Escape, the sidebar and the window
//! close button) asks before the values are thrown away.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

/// Set on the content of a page that holds input the user has not saved.
const DIRTY_CLASS: &str = "omagrow-dirty";

thread_local! {
    /// Whether the discard question is on screen already.
    static ASKING: Cell<bool> = const { Cell::new(false) };
}

/// One area of the sidebar: a list with pages pushed on top of it.
#[derive(Clone)]
pub(super) struct Section {
    nav: adw::NavigationView,
    root: adw::NavigationPage,
    refresh: Rc<dyn Fn()>,
}

impl Section {
    pub(super) fn new(
        nav: adw::NavigationView,
        root: adw::NavigationPage,
        refresh: Rc<dyn Fn()>,
    ) -> Self {
        Self { nav, root, refresh }
    }

    pub(super) fn widget(&self) -> &gtk::Widget {
        self.nav.upcast_ref()
    }

    /// Close every page above the list. The list is refreshed even when nothing was open, because
    /// another area may have changed what this one shows.
    pub(super) fn enter(&self) {
        if !self.nav.pop_to_page(&self.root) {
            (self.refresh)();
        }
    }

    /// Whether any page in this area holds input that is not saved.
    pub(super) fn is_dirty(&self) -> bool {
        let stack = self.nav.navigation_stack();
        (0..stack.n_items())
            .filter_map(|position| stack.item(position))
            .filter_map(|item| item.downcast::<adw::NavigationPage>().ok())
            .filter_map(|page| page.child())
            .any(|content| content.has_css_class(DIRTY_CLASS))
    }
}

/// The input of one form page.
///
/// [`Dirty::watch`] follows the fields after the form is built, so the values a form opens with do
/// not count as an edit. What the watcher cannot see (a chosen photo) is marked by hand with
/// [`Dirty::mark`].
///
/// The scope is weak: the field watchers live in the page and capture this flag, so a strong
/// reference back to the page would keep every closed form alive.
#[derive(Clone)]
pub(super) struct Dirty(Rc<Inner>);

struct Inner {
    scope: RefCell<Option<gtk::glib::WeakRef<gtk::Widget>>>,
}

impl Dirty {
    pub(super) fn new() -> Self {
        Self(Rc::new(Inner {
            scope: RefCell::new(None),
        }))
    }

    /// Follow a built form. `scope` is the page content, so the area can see the flag from the
    /// navigation stack.
    pub(super) fn watch(&self, scope: &impl IsA<gtk::Widget>) {
        let widget = scope.clone().upcast::<gtk::Widget>();
        watch_tree(&widget, self);
        *self.0.scope.borrow_mut() = Some(widget.downgrade());
    }

    pub(super) fn mark(&self) {
        if let Some(scope) = self.scope() {
            scope.add_css_class(DIRTY_CLASS);
        }
    }

    pub(super) fn is_dirty(&self) -> bool {
        self.scope()
            .is_some_and(|scope| scope.has_css_class(DIRTY_CLASS))
    }

    fn scope(&self) -> Option<gtk::Widget> {
        self.0
            .scope
            .borrow()
            .as_ref()
            .and_then(|scope| scope.upgrade())
    }
}

fn watch_tree(widget: &gtk::Widget, dirty: &Dirty) {
    let mark = {
        let dirty = dirty.clone();
        move || dirty.mark()
    };

    if let Some(calendar) = widget.downcast_ref::<gtk::Calendar>() {
        calendar.connect_day_selected({
            let mark = mark.clone();
            move |_| mark()
        });
    } else if let Some(row) = widget.downcast_ref::<adw::ComboRow>() {
        row.connect_selected_notify({
            let mark = mark.clone();
            move |_| mark()
        });
    } else if let Some(row) = widget.downcast_ref::<adw::SpinRow>() {
        row.connect_value_notify({
            let mark = mark.clone();
            move |_| mark()
        });
    } else if let Some(row) = widget.downcast_ref::<adw::SwitchRow>() {
        row.connect_active_notify({
            let mark = mark.clone();
            move |_| mark()
        });
    } else if let Some(dropdown) = widget.downcast_ref::<gtk::DropDown>() {
        dropdown.connect_selected_notify({
            let mark = mark.clone();
            move |_| mark()
        });
    } else if let Some(button) = widget.downcast_ref::<gtk::CheckButton>() {
        button.connect_toggled({
            let mark = mark.clone();
            move |_| mark()
        });
    } else if let Some(view) = widget.downcast_ref::<gtk::TextView>() {
        view.buffer().connect_changed({
            let mark = mark.clone();
            move |_| mark()
        });
    } else if let Some(editable) = widget.dynamic_cast_ref::<gtk::Editable>() {
        editable.connect_changed({
            let mark = mark.clone();
            move |_| mark()
        });
    }

    let mut child = widget.first_child();
    while let Some(current) = child {
        watch_tree(&current, dirty);
        child = current.next_sibling();
    }
}

/// The way out of a form page, and the flag that says it holds input.
///
/// Build one at the top of a form, use [`Guard::header`] for its header, call [`Guard::close`]
/// from Cancel and [`Guard::attach`] once the page exists.
#[derive(Clone)]
pub(super) struct Guard {
    dirty: Dirty,
    nav: adw::NavigationView,
}

impl Guard {
    pub(super) fn new(nav: &adw::NavigationView) -> Self {
        Self {
            dirty: Dirty::new(),
            nav: nav.clone(),
        }
    }

    /// A form header: no automatic back button, because that one cannot ask.
    pub(super) fn header(&self, title: &str) -> adw::HeaderBar {
        let guard = self.clone();
        form_header(title, Rc::new(move || guard.close()))
    }

    /// Back to the page below this form, asking first when fields were touched.
    pub(super) fn close(&self) {
        if !self.dirty.is_dirty() {
            self.nav.pop();
            return;
        }

        let Some(window) = window_of(&self.nav) else {
            return;
        };

        let nav = self.nav.clone();
        confirm_discard(
            &window,
            Rc::new(move || {
                nav.pop();
            }),
        );
    }

    /// Something other than closing the page, such as a reload, behind the same question.
    pub(super) fn ask(&self, action: Rc<dyn Fn()>) {
        if !self.dirty.is_dirty() {
            action();
            return;
        }

        let Some(window) = window_of(&self.nav) else {
            return;
        };

        confirm_discard(&window, action);
    }

    /// Mark the form by hand for something the watcher cannot see, such as a chosen photo.
    pub(super) fn mark(&self) {
        self.dirty.mark();
    }

    /// Block the silent exits, then follow the fields. Call after the form is built, so the
    /// values it opens with do not count as an edit.
    pub(super) fn attach(&self, page: &adw::NavigationPage, content: &impl IsA<gtk::Widget>) {
        page.set_can_pop(false);
        self.dirty.watch(content);

        let guard = self.clone();
        catch_escape(content, Rc::new(move || guard.close()));
    }
}

/// Ask before unsaved input is thrown away, then run `discard`.
pub(super) fn confirm_discard(window: &gtk::Window, discard: Rc<dyn Fn()>) {
    // A held-down Escape or a fast second click must not stack two questions, because the second
    // answer would close another page.
    if ASKING.with(|asking| asking.replace(true)) {
        return;
    }

    let dialog = adw::AlertDialog::builder()
        .heading("Discard your changes?")
        .body("The values in this form are not saved.")
        .build();
    dialog.add_response("keep", "Keep editing");
    dialog.add_response("discard", "Discard");
    dialog.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("keep"));
    dialog.set_close_response("keep");

    dialog.connect_response(None, move |_, response| {
        ASKING.with(|asking| asking.set(false));
        if response == "discard" {
            discard();
        }
    });

    // Present outside the key handler that may have asked for this: presenting a modal dialog
    // while an Escape is still being delivered lets the same key close it again.
    let window = window.clone();
    gtk::glib::idle_add_local_once(move || {
        dialog.present(Some(&window));
    });
}

/// The header of a form.
///
/// The automatic back button is off: it cannot ask before leaving. The button placed here runs the
/// same check as Cancel. Escape is caught on the page content by `catch_escape`.
fn form_header(title: &str, on_close: Rc<dyn Fn()>) -> adw::HeaderBar {
    let header = super::page_header(title);
    header.set_show_back_button(false);

    let back = gtk::Button::from_icon_name("go-previous-symbolic");
    back.add_css_class("flat");
    back.set_tooltip_text(Some("Back"));
    back.connect_clicked(move |_| on_close());
    header.pack_start(&back);

    header
}

/// Escape closes a form through the same check. Without this the page keeps its automatic Escape
/// behaviour, which pops without asking.
fn catch_escape(content: &impl IsA<gtk::Widget>, on_close: Rc<dyn Fn()>) {
    let scope = content.clone().upcast::<gtk::Widget>();
    // The controller is owned by `content`; a strong clone back to it would keep the whole page
    // alive. The weak handle is enough to find the window and its focus.
    let weak_scope = scope.downgrade();
    let controller = gtk::EventControllerKey::new();
    // Capture: a focused dropdown eats Escape before it bubbles up, and the form would have no
    // way out there.
    controller.set_propagation_phase(gtk::PropagationPhase::Capture);
    controller.connect_key_pressed(move |_, key, _, _| {
        if key != gtk::gdk::Key::Escape {
            return gtk::glib::Propagation::Proceed;
        }

        // Escape closes a popover first: a date picker or a menu is not a way out of the form.
        let in_popover = weak_scope
            .upgrade()
            .and_then(|scope| scope.root())
            .and_then(|root| root.downcast::<gtk::Window>().ok())
            .and_then(|window| gtk::prelude::GtkWindowExt::focus(&window))
            .is_some_and(|focus| {
                let mut widget = focus.parent();
                while let Some(parent) = widget {
                    if parent.is::<gtk::Popover>() {
                        return true;
                    }
                    widget = parent.parent();
                }
                false
            });

        if in_popover {
            return gtk::glib::Propagation::Proceed;
        }

        on_close();
        gtk::glib::Propagation::Stop
    });
    content.add_controller(controller);
}

fn window_of(nav: &adw::NavigationView) -> Option<gtk::Window> {
    nav.root()
        .and_then(|root| root.downcast::<gtk::Window>().ok())
}
