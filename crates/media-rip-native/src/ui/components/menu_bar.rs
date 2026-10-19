use gtk::{gio, prelude::*};

pub(crate) fn menu_bar() -> gtk::PopoverMenuBar {
    let menu = gio::Menu::new();

    let file = gio::Menu::new();
    file.append(Some("Open files…"), Some("win.open-file"));
    file.append(Some("Open folder…"), Some("win.open-folder"));
    file.append(Some("Open disc"), Some("win.open-disc"));
    file.append(Some("Eject disc"), Some("win.eject"));
    file.append(Some("Quit"), Some("win.quit"));
    menu.append_submenu(Some("File"), &file);

    let edit = gio::Menu::new();
    edit.append(Some("Settings…"), Some("win.preferences"));
    menu.append_submenu(Some("Edit"), &edit);

    let view = gio::Menu::new();
    view.append(Some("Refresh drives"), Some("win.refresh"));
    view.append(Some("Clear messages"), Some("win.clear-messages"));
    menu.append_submenu(Some("View"), &view);

    let help = gio::Menu::new();
    help.append(Some("About MediaRip"), Some("win.about"));
    menu.append_submenu(Some("Help"), &help);

    let bar = gtk::PopoverMenuBar::from_model(Some(&menu));
    bar.add_css_class("menu-strip");
    bar
}
