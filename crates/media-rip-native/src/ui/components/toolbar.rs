use gtk::{gio, prelude::*};

#[derive(Debug, Clone)]
pub(crate) struct Toolbar {
    pub root: gtk::Box,
    pub open_file: gtk::Button,
    pub open_folder: gtk::Button,
    pub open_disc: gtk::Button,
    pub eject: gtk::Button,
    pub refresh: gtk::Button,
    pub preferences: gtk::Button,
}

impl Toolbar {
    pub fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let menus = menu_bar();
        menus.add_css_class("menu-strip");
        root.append(&menus);

        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        buttons.add_css_class("main-toolbar");
        let open_file = tool_button("document-open-symbolic", "Open files");
        let open_folder = tool_button("folder-open-symbolic", "Open folder");
        let open_disc = tool_button("media-optical-symbolic", "Open disc");
        let eject = tool_button("media-eject-symbolic", "Eject disc");
        let refresh = tool_button("view-refresh-symbolic", "Refresh drives");
        let preferences = tool_button("preferences-system-symbolic", "Preferences");
        buttons.append(&open_file);
        buttons.append(&open_folder);
        buttons.append(&separator());
        buttons.append(&open_disc);
        buttons.append(&eject);
        buttons.append(&separator());
        buttons.append(&refresh);
        buttons.append(&separator());
        buttons.append(&preferences);
        root.append(&buttons);

        Self {
            root,
            open_file,
            open_folder,
            open_disc,
            eject,
            refresh,
            preferences,
        }
    }
}

fn menu_bar() -> gtk::PopoverMenuBar {
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

    gtk::PopoverMenuBar::from_model(Some(&menu))
}

fn tool_button(icon: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::builder()
        .icon_name(icon)
        .tooltip_text(tooltip)
        .build();
    button.add_css_class("flat");
    button.add_css_class("toolbar-button");
    button
}

fn separator() -> gtk::Separator {
    gtk::Separator::new(gtk::Orientation::Vertical)
}
