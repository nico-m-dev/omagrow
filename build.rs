fn main() {
    glib_build_tools::compile_resources(
        &["assets"],
        "assets/omagrow.gresource.xml",
        "omagrow.gresource",
    );
}
