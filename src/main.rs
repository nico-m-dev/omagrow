mod attribute_seeds;
mod calendar;
mod changes;
mod cli;
mod climate;
mod clock;
mod completeness;
mod date_format;
mod daylight;
mod frost;
mod frost_lookup;
mod geocode;
mod media;
mod observation;
mod online;
mod plant_data;
mod planting;
mod proposal;
mod provenance;
mod schedule;
mod store;
mod theme;
mod ui;
mod units;
mod weather;

use gtk::prelude::*;
use libadwaita as adw;

const APP_ID: &str = "io.github.nicomdev.OmaGrow";
const ICONS: &str = "/io/github/nicomdev/OmaGrow/icons";

fn main() -> gtk::glib::ExitCode {
    // A command exits here; only an empty argument list starts the window.
    cli::run_if_command(&std::env::args().skip(1).collect::<Vec<_>>());

    gtk::gio::resources_register_include!("omagrow.gresource")
        .expect("failed to register bundled resources");

    // A different id lets a second instance run next to the real one while developing.
    let app_id = std::env::var("OMAGROW_APP_ID").unwrap_or_else(|_| APP_ID.to_string());

    let app = adw::Application::builder().application_id(&app_id).build();

    app.connect_startup(|_| {
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::IconTheme::for_display(&display).add_resource_path(ICONS);
        }
    });

    app.connect_activate(ui::build);

    app.run()
}
