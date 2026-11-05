//! The list page behind a profile editor: one boxed list, an Add button and a refresh on show.

use std::rc::Rc;

use gtk::prelude::*;
use libadwaita as adw;

pub(in crate::ui) fn page(
    title: &str,
    add_label: &str,
    on_add: Rc<dyn Fn()>,
    refresh: Rc<dyn Fn(&gtk::ListBox)>,
) -> adw::NavigationPage {
    let header = crate::ui::page_header(title);

    let add = gtk::Button::with_label(add_label);
    add.add_css_class("suggested-action");
    header.pack_end(&add);

    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .build();
    list.add_css_class("boxed-list");

    let content = gtk::Box::new(gtk::Orientation::Vertical, 18);
    content.set_margin_top(24);
    content.set_margin_bottom(32);
    content.set_margin_start(24);
    content.set_margin_end(24);
    content.append(&list);

    let clamp = adw::Clamp::new();
    clamp.set_maximum_size(720);
    clamp.set_tightening_threshold(900);
    clamp.set_child(Some(&content));

    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&clamp)
        .build();

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&scroller));

    let page = adw::NavigationPage::new(&toolbar, title);

    add.connect_clicked(move |_| on_add());

    refresh(&list);
    scroller.connect_map({
        let list = list.clone();
        let refresh = refresh.clone();
        move |_| refresh(&list)
    });

    page
}
