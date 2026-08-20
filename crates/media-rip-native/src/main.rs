mod app;
mod logging;
mod ui;

use gtk::prelude::*;

fn main() -> gtk::glib::ExitCode {
    logging::init();
    let application = gtk::Application::builder()
        .application_id("io.github.mediarip.MediaRip")
        .flags(gtk::gio::ApplicationFlags::HANDLES_OPEN)
        .build();
    application.connect_activate(|application| app::launch(application, None));
    application.connect_open(|application, files, _| {
        let path = files.first().and_then(|file| file.path());
        app::launch(application, path);
    });
    application.run()
}
