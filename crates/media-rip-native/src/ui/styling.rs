use gtk::{CssProvider, STYLE_PROVIDER_PRIORITY_APPLICATION, gdk};

const APP_CSS: &str = r#"
window {
  background-color: @theme_bg_color;
  color: @theme_fg_color;
}

.menu-strip {
  background-color: @theme_bg_color;
  color: @theme_fg_color;
  border-bottom: 1px solid @borders;
}

.main-toolbar {
  min-height: 45px;
  padding: 3px 7px;
  background-color: @theme_bg_color;
  border-bottom: 1px solid @borders;
}

.toolbar-button {
  min-width: 34px;
  min-height: 34px;
  padding: 2px;
}

.source-strip {
  padding: 5px 9px;
  background-color: @theme_bg_color;
  color: @theme_fg_color;
  border-bottom: 1px solid @borders;
}

.source-label {
  font-weight: 700;
}

.content-surface {
  background-color: @theme_base_color;
  color: @theme_text_color;
}

.panel {
  background-color: @theme_base_color;
  color: @theme_text_color;
  border: 1px solid @borders;
  border-radius: 5px;
}

.panel-heading {
  min-height: 25px;
  padding: 3px 8px;
  font-weight: 700;
  color: @theme_fg_color;
  background-color: @theme_bg_color;
  border-bottom: 1px solid @borders;
}

.title-row {
  min-height: 40px;
  padding: 4px 7px;
  color: @theme_text_color;
  background-color: @theme_base_color;
  border-bottom: 1px solid alpha(@borders, 0.55);
}

.title-row:hover,
.track-row:hover {
  background-color: alpha(@theme_selected_bg_color, 0.18);
}

.track-row {
  min-height: 30px;
  padding: 1px 5px 1px 37px;
  color: @theme_text_color;
  background-color: @theme_base_color;
  border-bottom: 1px solid alpha(@borders, 0.4);
}

.row-title {
  font-weight: normal;
}

.main-title-badge {
  min-height: 18px;
  padding: 1px 8px;
  border-radius: 10px;
  background-color: @theme_selected_bg_color;
  color: @theme_selected_fg_color;
  font-size: 0.76em;
  font-weight: 700;
}

.row-detail {
  color: alpha(@theme_text_color, 0.72);
  font-size: 0.92em;
}

.info-grid {
  padding: 9px;
}

.info-label {
  color: alpha(@theme_text_color, 0.72);
}

.info-value {
  color: @theme_text_color;
}

.output-panel {
  background-color: @theme_bg_color;
}

.make-mkv-button {
  min-width: 128px;
  min-height: 86px;
  padding: 10px 14px;
  font-weight: 700;
  font-size: 1.05em;
  border-radius: 5px;
}

.idle-disc-button {
  min-width: 160px;
  min-height: 160px;
  padding: 18px;
  border-radius: 8px;
}

.idle-title {
  font-size: 1.25em;
  font-weight: 700;
}

.idle-caption {
  color: alpha(@theme_fg_color, 0.72);
}

.message-view {
  font-family: monospace;
  font-size: 0.93em;
  background-color: @theme_base_color;
  color: @theme_text_color;
}

.progress-area {
  padding: 7px 10px;
  background-color: @theme_bg_color;
  color: @theme_fg_color;
  border-top: 1px solid @borders;
}

.status-bar {
  min-height: 24px;
  padding: 2px 8px;
  color: @theme_fg_color;
  background-color: @theme_bg_color;
  border-top: 1px solid @borders;
}

.status-error {
  color: @error_color;
}

.status-success {
  color: @success_color;
}

.settings-root {
  min-width: 820px;
  min-height: 550px;
  background-color: @theme_base_color;
  color: @theme_text_color;
}

.settings-dialog-action {
  margin-bottom: 12px;
}

.settings-dialog-primary-action {
  margin-right: 12px;
}

.settings-sidebar {
  min-width: 205px;
  padding: 14px 12px;
  background-color: @theme_bg_color;
  color: @theme_fg_color;
}

.settings-sidebar-title {
  margin: 0 8px 8px;
  font-size: 1.18em;
  font-weight: 700;
}

.settings-nav-group {
  margin: 8px 9px 3px;
  color: alpha(@theme_fg_color, 0.58);
  font-size: 0.78em;
  font-weight: 700;
}

.settings-nav-button {
  min-height: 38px;
  padding: 5px 9px;
  border: 0;
  border-radius: 7px;
  background: transparent;
  box-shadow: none;
}

.settings-nav-button:hover {
  background-color: alpha(@theme_selected_bg_color, 0.12);
}

.settings-nav-button:checked {
  background-color: alpha(@theme_selected_bg_color, 0.23);
  color: @theme_selected_fg_color;
}

.settings-nav-title {
  font-weight: 600;
}

.settings-nav-detail {
  color: alpha(currentColor, 0.68);
  font-size: 0.86em;
}

.settings-page {
  padding: 28px 34px 36px;
  background-color: @theme_base_color;
  color: @theme_text_color;
}

.settings-page-title {
  font-size: 1.65em;
  font-weight: 700;
}

.settings-page-subtitle {
  color: alpha(@theme_text_color, 0.68);
  font-size: 1.02em;
}

.settings-badge {
  margin-top: -8px;
  padding: 3px 8px;
  border-radius: 999px;
  color: @theme_selected_fg_color;
  background-color: alpha(@theme_selected_bg_color, 0.75);
  font-size: 0.82em;
  font-weight: 700;
}

.settings-section {
  border: 1px solid @borders;
  border-radius: 9px;
  background-color: @theme_bg_color;
}

.settings-section-header {
  padding: 13px 16px 11px;
  border-bottom: 1px solid alpha(@borders, 0.75);
  background-color: alpha(@theme_selected_bg_color, 0.055);
}

.settings-section-title {
  font-size: 1.04em;
  font-weight: 700;
}

.settings-row-title {
  font-weight: 600;
}

.settings-section-subtitle,
.settings-row-detail {
  color: alpha(@theme_fg_color, 0.67);
  font-size: 0.9em;
}

.settings-row {
  min-height: 52px;
  padding: 11px 16px;
  border-bottom: 1px solid alpha(@borders, 0.55);
}

.settings-row:last-child {
  border-bottom: 0;
}

.settings-note,
.settings-planned-note,
.settings-warning-note {
  padding: 11px 13px;
  border: 1px solid alpha(@theme_selected_bg_color, 0.45);
  border-radius: 8px;
  background-color: alpha(@theme_selected_bg_color, 0.09);
}

.settings-planned-note {
  border-color: alpha(@warning_color, 0.45);
  background-color: alpha(@warning_color, 0.08);
}

.settings-warning-note {
  border-color: alpha(@warning_color, 0.55);
  background-color: alpha(@warning_color, 0.1);
}
"#;

pub(crate) fn install() {
    let provider = CssProvider::new();
    provider.load_from_data(APP_CSS);
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::APP_CSS;

    #[test]
    fn stylesheet_uses_the_gtk_theme_palette() {
        assert!(APP_CSS.contains("@theme_bg_color"));
        assert!(APP_CSS.contains("@theme_fg_color"));
        assert!(APP_CSS.contains("@theme_base_color"));
        assert!(APP_CSS.contains("@theme_text_color"));
        assert!(
            !APP_CSS.contains('#'),
            "application colors must come from the active GTK theme"
        );
    }
}
