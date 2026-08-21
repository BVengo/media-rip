use gtk::prelude::*;

mod components;
mod i18n;
mod styling;
mod window;

pub(crate) use components::SettingsDialog;
pub(crate) use components::{ContentAction, DiscUiState, InfoAction};
pub(crate) use i18n::translate;
pub(crate) use window::MainWindow;

pub(crate) fn install_styles() {
    styling::install();
}

pub(crate) fn space_dialog_actions<W: IsA<gtk::Dialog>>(
    dialog: &W,
    responses: &[gtk::ResponseType],
) {
    let dialog = dialog.upcast_ref::<gtk::Dialog>();
    for (index, response) in responses.iter().enumerate() {
        if let Some(button) = dialog.widget_for_response(*response) {
            button.set_margin_top(8);
            button.set_margin_bottom(12);
            if index + 1 == responses.len() {
                button.set_margin_end(12);
            }
        }
    }
}
