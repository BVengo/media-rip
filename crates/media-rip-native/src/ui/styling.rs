use gtk::{CssProvider, STYLE_PROVIDER_PRIORITY_APPLICATION, gdk};

const APP_CSS: &str = r#"
window {
  background-color: @theme_bg_color;
  color: @theme_fg_color;
}

/* Explicit application colours otherwise override the theme's disabled text.
   Remove inherited text shadows so every glyph uses the same disabled colour. */
window label:disabled,
window entry:disabled,
window text:disabled {
  color: @insensitive_fg_color;
  text-shadow: none;
}

window scrollbar.vertical slider {
  min-width: 10px;
}

window scrollbar.horizontal slider {
  min-height: 10px;
}

.menu-strip {
  background-color: @theme_bg_color;
  color: @theme_fg_color;
  border-bottom: 1px solid @borders;
}

popover.menu contents {
  padding: 2px;
}

popover.menu modelbutton {
  min-height: 0;
  padding: 6px 10px;
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

.workflow-bar {
  padding: 0;
  background-color: alpha(@theme_bg_color, 0.7);
  border-bottom: 1px solid @borders;
}

.workflow-button {
  min-width: 0;
  min-height: 40px;
  padding: 3px 12px;
  border: 0;
  border-radius: 0;
  background: transparent;
  box-shadow: none;
}

.workflow-button:hover {
  background-color: alpha(@theme_selected_bg_color, 0.10);
}

.workflow-button.active {
  background-color: alpha(@theme_selected_bg_color, 0.16);
  box-shadow: inset 0 -3px @theme_selected_bg_color;
}

.workflow-stage {
  padding: 0 4px;
}

.workflow-number {
  min-width: 24px;
  min-height: 24px;
  padding: 0;
  border-radius: 999px;
  color: @theme_selected_fg_color;
  background-color: @theme_selected_bg_color;
  font-weight: 700;
}

.workflow-header {
  padding: 20px 24px 12px;
  background-color: @theme_base_color;
}

.workflow-navigation {
  padding: 10px 20px;
  border-top: 1px solid @borders;
  background-color: @theme_bg_color;
}

.metadata-editor {
  padding: 18px;
}

.metadata-item {
  margin: 8px;
  padding: 12px;
  border: 1px solid @borders;
  border-radius: 8px;
  background-color: @theme_bg_color;
}

.workflow-page {
  padding: 10px;
  background-color: @theme_base_color;
  color: @theme_text_color;
}

.source-picker {
  min-width: 620px;
  padding: 24px;
}

.source-select {
  min-height: 42px;
}

.source-select button {
  min-height: 40px;
  padding: 4px 12px;
  background-color: @theme_base_color;
  color: @theme_text_color;
}

.source-identify-button {
  min-height: 82px;
  padding: 10px 18px;
}

.source-identify-title {
  font-size: 1.12em;
  font-weight: 700;
}

.source-identify-detail {
  color: alpha(currentColor, 0.78);
  font-size: 0.92em;
}

.identification-row {
  padding-top: 12px;
  border-top: 1px solid alpha(@borders, 0.7);
}

.metadata-workspace,
.output-review {
  min-height: 180px;
}

.metadata-controls,
.metadata-actions {
  min-height: 44px;
  padding: 6px 10px;
  background-color: @theme_bg_color;
}

.metadata-controls {
  border-bottom: 1px solid @borders;
}

.metadata-actions {
  border-top: 1px solid @borders;
}

.metadata-scroll {
  margin-right: 4px;
}

.inline-empty-state {
  min-height: 150px;
  padding: 20px;
}

.workflow-card-title {
  font-size: 1.15em;
  font-weight: 700;
}

.output-layout {
  background-color: @theme_base_color;
  border: 1px solid @borders;
  border-radius: 6px;
}

.output-config {
  background-color: @theme_bg_color;
  border-radius: 0 5px 5px 0;
}

.output-layout > separator {
  min-width: 1px;
  background-color: @borders;
}

.output-actions {
  padding: 12px;
  border-top: 1px solid @borders;
  background-color: @theme_bg_color;
  border-radius: 0 0 5px 0;
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
  border-radius: 4px 4px 0 0;
}

.panel-tabs {
  min-height: 31px;
  padding: 2px 5px;
  background-color: @theme_bg_color;
  border-bottom: 1px solid @borders;
  border-radius: 4px 4px 0 0;
}

.panel-tabs button {
  min-height: 25px;
  min-width: 0;
  padding: 2px 6px;
}

.title-row {
  min-height: 40px;
  padding: 4px 7px;
  color: @theme_text_color;
  background-color: @theme_base_color;
  border-bottom: 1px solid alpha(@borders, 0.55);
}

.title-expander {
  min-width: 28px;
  min-height: 28px;
  padding: 0;
}

.select-all-row {
  min-height: 40px;
  padding: 4px 11px;
  color: @theme_text_color;
  background-color: alpha(@theme_bg_color, 0.7);
  border-bottom: 1px solid @borders;
}

.title-row:hover,
.track-row:hover {
  background-color: alpha(@theme_selected_bg_color, 0.18);
}

.title-row.viewing-row,
.track-row.viewing-row {
  color: @theme_text_color;
  background-color: alpha(@theme_selected_bg_color, 0.3);
  box-shadow: inset 3px 0 @theme_selected_bg_color;
}

.title-row.viewing-row:hover,
.track-row.viewing-row:hover {
  background-color: alpha(@theme_selected_bg_color, 0.38);
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

.duplicate-title-badge {
  min-height: 18px;
  padding: 1px 8px;
  border: 1px solid alpha(@warning_color, 0.55);
  border-radius: 10px;
  background-color: alpha(@warning_color, 0.14);
  color: @warning_color;
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

.tags-content {
  padding: 9px;
}

.episode-assignment-row {
  padding-top: 8px;
}

.tags-warning {
  padding: 6px 8px;
  color: @warning_color;
  background-color: alpha(@warning_color, 0.09);
  border: 1px solid alpha(@warning_color, 0.35);
  border-radius: 4px;
}

.add-tag-row {
  margin-top: 4px;
  padding-top: 9px;
  border-top: 1px solid alpha(@borders, 0.6);
}

.artwork-control {
  border: 1px solid alpha(@borders, 0.75);
  border-radius: 4px;
  background-color: alpha(@theme_fg_color, 0.04);
}

.cover-art,
.cover-art-placeholder {
  border-radius: 3px;
}

.artwork-overlay-actions {
  padding: 2px;
  border-radius: 5px;
  background-color: alpha(@theme_bg_color, 0.88);
}

.artwork-overlay-actions button {
  min-width: 28px;
  min-height: 28px;
  padding: 2px;
  border-radius: 4px;
  background-color: alpha(@theme_base_color, 0.9);
}

.artwork-overlay-actions button:hover {
  background-color: @theme_selected_bg_color;
  color: @theme_selected_fg_color;
}

.config-label {
  font-weight: 600;
}

.output-panel {
  background-color: @theme_bg_color;
}

.make-mkv-button {
  min-width: 128px;
  min-height: 36px;
  padding: 5px 12px;
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

.activity-panel {
  margin-bottom: 4px;
  border: 1px solid @borders;
  border-radius: 5px;
}

.activity-toggle {
  min-height: 30px;
  margin: 4px;
  padding: 3px 9px;
  font-weight: 600;
}

.progress-area {
  padding: 5px 10px;
  background-color: @theme_bg_color;
  color: @theme_fg_color;
  border-top: 1px solid @borders;
}

.preflight-title {
  font-size: 1.25em;
  font-weight: 700;
}

.preflight-warning {
  padding: 8px 10px;
  color: @warning_color;
  background-color: alpha(@warning_color, 0.09);
  border: 1px solid alpha(@warning_color, 0.35);
  border-radius: 5px;
}

.preflight-files {
  border: 1px solid @borders;
  border-radius: 6px;
  background-color: @theme_base_color;
}

.preflight-file-row {
  padding: 8px 10px;
  border-bottom: 1px solid alpha(@borders, 0.55);
}

.preflight-file-row:last-child {
  border-bottom: 0;
}

.preflight-file-row:first-child {
  border-radius: 5px 5px 0 0;
}

.preflight-file-row:last-child {
  border-radius: 0 0 5px 5px;
}

.preflight-file-row:only-child {
  border-radius: 5px;
}

.output-review {
  padding: 12px;
  background-color: @theme_base_color;
  border-radius: 5px 0 0 5px;
}

.output-job {
  padding: 10px 12px;
  border: 1px solid @borders;
  border-radius: 6px;
  background-color: @theme_bg_color;
}

.output-job-status {
  font-weight: 700;
}

.output-file-status {
  color: alpha(@theme_text_color, 0.78);
  font-size: 0.92em;
}

.output-file-complete {
  background-color: alpha(@success_color, 0.08);
}

.output-cancel {
  min-height: 28px;
  padding: 2px 10px;
}

.metadata-review-row {
  padding: 9px 10px;
  border-bottom: 1px solid alpha(@borders, 0.55);
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
  border-radius: 8px 8px 0 0;
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
  border-radius: 0 0 8px 8px;
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
