//! Page chrome shared by more than one screen: a missing record and a delete confirmation.

use std::rc::Rc;

use gtk::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

/// The page a caller shows when the record it was opened for no longer exists.
pub(in crate::ui) fn missing_page(
    header_title: &str,
    page_title: &str,
    heading: &str,
    description: Option<&str>,
) -> adw::NavigationPage {
    let mut status = adw::StatusPage::builder()
        .icon_name("dialog-error-symbolic")
        .title(heading);
    if let Some(description) = description {
        status = status.description(description);
    }
    let status = status.build();

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&crate::ui::page_header(header_title));
    toolbar.set_content(Some(&status));

    adw::NavigationPage::new(&toolbar, page_title)
}

/// Asks before a destructive action. `parent` only locates the window to present over.
pub(in crate::ui) fn confirm_delete(
    parent: &impl IsA<gtk::Widget>,
    heading: &str,
    body: &str,
    run: Rc<dyn Fn()>,
) {
    let Some(window) = parent
        .clone()
        .upcast::<gtk::Widget>()
        .root()
        .and_then(|root| root.downcast::<gtk::Window>().ok())
    else {
        return;
    };

    let dialog = adw::AlertDialog::builder()
        .heading(heading)
        .body(body)
        .build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("delete", "Delete");
    dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");

    dialog.connect_response(None, move |_, response| {
        if response == "delete" {
            run();
        }
    });

    dialog.present(Some(&window));
}
