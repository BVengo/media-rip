use gtk::prelude::*;

const PROJECT_URL: &str = "https://github.com/BVengo/media-rip";
const TMDB_URL: &str = "https://www.themoviedb.org";
const TVDB_URL: &str = "https://thetvdb.com";
const TVMAZE_URL: &str = "https://www.tvmaze.com";

pub(crate) fn present(parent: &gtk::ApplicationWindow) {
    let dialog = gtk::Dialog::builder()
        .title("About MediaRip")
        .transient_for(parent)
        .modal(true)
        .default_width(720)
        .default_height(560)
        .build();
    dialog.add_button("Close", gtk::ResponseType::Close);
    crate::ui::space_dialog_actions(&dialog, &[gtk::ResponseType::Close]);
    dialog.connect_response(|dialog, _| dialog.close());

    let root = gtk::Box::new(gtk::Orientation::Vertical, 12);
    root.set_margin_top(16);
    root.set_margin_bottom(12);
    root.set_margin_start(16);
    root.set_margin_end(16);

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    header.set_halign(gtk::Align::Center);
    header.set_size_request(620, -1);

    let app_icon = svg_picture(
        include_bytes!("../../../../packaging/linux/io.github.mediarip.MediaRip.svg"),
        "MediaRip",
        80,
        80,
    );
    app_icon.set_valign(gtk::Align::Center);
    header.append(&app_icon);

    let details = gtk::Box::new(gtk::Orientation::Vertical, 4);
    details.set_hexpand(true);

    let title = gtk::Label::new(None);
    title.set_markup(&format!(
        "<span size=\"x-large\" weight=\"bold\">MediaRip</span>  Version {}",
        env!("CARGO_PKG_VERSION")
    ));
    title.set_xalign(0.0);
    details.append(&title);

    let description = gtk::Label::new(Some(
        "A native Linux application for inspecting, preserving and encoding optical media.",
    ));
    description.set_wrap(true);
    description.set_xalign(0.0);
    details.append(&description);
    let project_link = gtk::LinkButton::with_label(PROJECT_URL, "MediaRip on GitHub");
    project_link.add_css_class("flat");
    project_link.set_halign(gtk::Align::Start);
    details.append(&project_link);
    header.append(&details);
    root.append(&header);

    let notebook = gtk::Notebook::new();
    notebook.set_vexpand(true);
    notebook.append_page(&credits_page(), Some(&gtk::Label::new(Some("Credits"))));
    notebook.append_page(
        &legal_text_page(include_str!("../../../../LICENSE")),
        Some(&gtk::Label::new(Some("Licence"))),
    );
    notebook.append_page(
        &legal_text_page(include_str!("../../../../THIRD_PARTY_NOTICES.txt")),
        Some(&gtk::Label::new(Some("Third-party licences"))),
    );
    root.append(&notebook);

    dialog.content_area().append(&root);
    dialog.present();
}

fn credits_page() -> gtk::ScrolledWindow {
    let credits = gtk::Box::new(gtk::Orientation::Vertical, 10);
    credits.set_halign(gtk::Align::Center);
    credits.set_size_request(620, -1);
    credits.set_margin_top(16);
    credits.set_margin_bottom(16);
    credits.set_margin_start(16);
    credits.set_margin_end(16);

    let authors = gtk::Label::new(Some("Copyright (C) 2026 MediaRip contributors"));
    authors.set_xalign(0.0);
    credits.append(&authors);

    let heading = gtk::Label::new(None);
    heading.set_markup("<span size=\"large\" weight=\"bold\">Metadata and services</span>");
    heading.set_xalign(0.0);
    credits.append(&heading);

    let tmdb_link = gtk::LinkButton::builder().uri(TMDB_URL).build();
    tmdb_link.add_css_class("flat");
    tmdb_link.set_halign(gtk::Align::Start);
    tmdb_link.set_child(Some(&tmdb_logo()));
    tmdb_link.set_tooltip_text(Some("The Movie Database (TMDB)"));
    credits.append(&tmdb_link);
    credits.append(&credit_text(
        "This product uses the TMDB API but is not endorsed or certified by TMDB.",
    ));

    credits.append(&provider_credit(
        "TheTVDB",
        TVDB_URL,
        "Metadata provided by TheTVDB. Please consider adding missing information or subscribing.",
    ));
    credits.append(&provider_credit(
        "TVMaze",
        TVMAZE_URL,
        "TV metadata provided by TVMaze under CC BY-SA.",
    ));
    credits.append(&provider_credit(
        "MusicBrainz",
        "https://musicbrainz.org",
        "Audio metadata provided by MusicBrainz.",
    ));
    credits.append(&provider_credit(
        "Cover Art Archive",
        "https://coverartarchive.org",
        "Artwork is supplied by the Cover Art Archive and remains subject to its owners' rights.",
    ));
    credits.append(&provider_credit(
        "TheDiscDB",
        "https://thediscdb.com",
        "Optical-disc fingerprint matches provided by TheDiscDB.",
    ));
    credits.append(&provider_credit(
        "ntfy",
        "https://ntfy.sh",
        "Optional push notifications are delivered through ntfy or a user-selected ntfy server.",
    ));

    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .child(&credits)
        .build()
}

fn tmdb_logo() -> gtk::Picture {
    svg_picture(include_bytes!("../../assets/tmdb.svg"), "TMDB", 208, 27)
}

fn svg_picture(bytes: &[u8], alternative_text: &str, width: i32, height: i32) -> gtk::Picture {
    let loader = gdk_pixbuf::PixbufLoader::new();
    loader.set_size(width, height);
    loader
        .write(bytes)
        .expect("the embedded logo should be valid SVG");
    loader
        .close()
        .expect("the embedded logo should finish decoding");
    let pixbuf = loader
        .pixbuf()
        .expect("the embedded logo should produce an image");
    let picture = gtk::Picture::for_pixbuf(&pixbuf);
    picture.set_alternative_text(Some(alternative_text));
    picture.set_size_request(width, height);
    picture
}

fn provider_credit(name: &str, uri: &str, description: &str) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Vertical, 1);
    let link = gtk::LinkButton::with_label(uri, name);
    link.add_css_class("flat");
    link.set_halign(gtk::Align::Start);
    row.append(&link);
    row.append(&credit_text(description));
    row
}

fn credit_text(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .wrap(true)
        .selectable(true)
        .xalign(0.0)
        .build()
}

fn legal_text_page(text: &str) -> gtk::ScrolledWindow {
    let view = gtk::TextView::builder()
        .editable(false)
        .cursor_visible(false)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::WordChar)
        .left_margin(16)
        .right_margin(16)
        .top_margin(16)
        .bottom_margin(16)
        .build();
    view.buffer().set_text(text);

    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .child(&view)
        .build()
}
