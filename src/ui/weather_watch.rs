//! Tells an open page that a weather sync landed.
//!
//! The weather data is loaded when a page is built. Without this, a page that is already on screen
//! keeps showing "no data" after a successful background sync until the user navigates away and
//! back. A page registers one callback while it lives; the list holds only weak references, so a
//! closed page disappears on its own.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

type Listener = Rc<dyn Fn()>;

thread_local! {
    static LISTENERS: RefCell<Vec<Weak<dyn Fn()>>> = const { RefCell::new(Vec::new()) };
}

/// Keep `listener` alive with the page that owns it.
pub(super) fn register(listener: Listener) {
    LISTENERS.with(|listeners| {
        let mut listeners = listeners.borrow_mut();
        listeners.retain(|weak| weak.strong_count() > 0);

        let known = listeners
            .iter()
            .filter_map(Weak::upgrade)
            .any(|existing| Rc::ptr_eq(&existing, &listener));
        if !known {
            listeners.push(Rc::downgrade(&listener));
        }
    });
}

/// Run every live listener on the main thread, after a successful sync.
pub(super) fn notify() {
    let callbacks: Vec<Listener> = LISTENERS.with(|listeners| {
        let mut listeners = listeners.borrow_mut();
        listeners.retain(|weak| weak.strong_count() > 0);
        listeners.iter().filter_map(Weak::upgrade).collect()
    });

    for callback in callbacks {
        callback();
    }
}
