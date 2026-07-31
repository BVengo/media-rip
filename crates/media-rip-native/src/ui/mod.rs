mod components;
mod i18n;
mod styling;
mod window;

pub(crate) use components::ContentAction;
pub(crate) use components::SettingsDialog;
pub(crate) use i18n::translate;
pub(crate) use window::MainWindow;

pub(crate) fn install_styles() {
    styling::install();
}
