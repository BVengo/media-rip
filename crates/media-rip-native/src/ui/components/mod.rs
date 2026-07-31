mod content_tree;
mod info_panel;
mod messages;
mod settings_dialog;
mod sidebar;
mod source_bar;
mod toolbar;

pub(crate) use content_tree::{ContentAction, ContentTree};
pub(crate) use info_panel::InfoPanel;
pub(crate) use messages::MessagesPanel;
pub(crate) use settings_dialog::SettingsDialog;
pub(crate) use sidebar::Sidebar;
pub(crate) use source_bar::SourceBar;
pub(crate) use toolbar::Toolbar;

use gtk::prelude::*;

pub(crate) fn panel(title: &str, child: &impl IsA<gtk::Widget>) -> gtk::Box {
    let container = gtk::Box::new(gtk::Orientation::Vertical, 0);
    container.add_css_class("panel");
    let heading = gtk::Label::new(Some(title));
    heading.set_xalign(0.0);
    heading.add_css_class("panel-heading");
    container.append(&heading);
    child.set_hexpand(true);
    child.set_vexpand(true);
    container.append(child);
    container
}
