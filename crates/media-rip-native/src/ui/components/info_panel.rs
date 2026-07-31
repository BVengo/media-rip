use gtk::prelude::*;
use media_rip_core::{
    InfoField, MediaCatalog,
    util::{format_bytes, format_duration},
};

use crate::app::Selection;

#[derive(Debug, Clone)]
pub(crate) struct InfoPanel {
    pub root: gtk::Box,
    grid: gtk::Grid,
}

impl InfoPanel {
    pub fn new() -> Self {
        let grid = gtk::Grid::builder()
            .column_spacing(14)
            .row_spacing(6)
            .hexpand(true)
            .vexpand(false)
            .build();
        grid.add_css_class("info-grid");
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .min_content_height(72)
            .hexpand(true)
            .vexpand(true)
            .child(&grid)
            .build();
        let root = super::panel("Info", &scroll);
        let panel = Self { root, grid };
        panel.render_fields(&[
            InfoField::new("Type", "No source"),
            InfoField::new("Name", "Insert or open a disc to begin"),
        ]);
        panel
    }

    pub fn render(&self, catalog: Option<&MediaCatalog>, selection: &Selection) {
        let Some(catalog) = catalog else {
            self.render_fields(&[
                InfoField::new("Type", "No source"),
                InfoField::new("Name", "Insert or open a disc to begin"),
            ]);
            return;
        };

        match selection {
            Selection::Source => self.render_fields(&catalog.source_info),
            Selection::Title(id) => {
                let Some(title) = catalog.titles.iter().find(|title| title.id == *id) else {
                    self.render_fields(&catalog.source_info);
                    return;
                };
                let mut fields = vec![
                    InfoField::new("Type", "Media item"),
                    InfoField::new("Name", &title.name),
                    InfoField::new("Source item ID", &title.source_title_id),
                    InfoField::new("Duration", format_duration(title.duration_ms)),
                    InfoField::new("Chapters", title.chapters.len().to_string()),
                ];
                if title.is_main_title {
                    fields.push(InfoField::new(
                        "Blu-ray role",
                        "Main title (identified by disc metadata)",
                    ));
                }
                if let Some(size) = title.size_bytes {
                    let size = format_bytes(size);
                    fields.push(InfoField::new(
                        if title.size_is_estimate {
                            "Estimated size"
                        } else {
                            "Size"
                        },
                        size,
                    ));
                }
                if let Some(count) = title.segment_count {
                    fields.push(InfoField::new("Segment count", count.to_string()));
                }
                if let Some(map) = &title.segment_map {
                    fields.push(InfoField::new("Segment map", map));
                }
                if let Some(ranges) = title
                    .duplicate_key
                    .as_deref()
                    .and_then(format_playlist_ranges)
                {
                    fields.push(InfoField::new("Exact clip ranges", ranges));
                }
                fields.push(InfoField::new("File name", &title.output_file));
                fields.push(InfoField::new(
                    "Selected tracks",
                    title.selected_track_count().to_string(),
                ));
                self.render_fields(&fields);
            }
            Selection::Track {
                title_id,
                stream_index,
            } => {
                let track = catalog
                    .titles
                    .iter()
                    .find(|title| title.id == *title_id)
                    .and_then(|title| {
                        title
                            .tracks
                            .iter()
                            .find(|track| track.index == *stream_index)
                    });
                let Some(track) = track else {
                    self.render_fields(&catalog.source_info);
                    return;
                };
                let mut fields = vec![
                    InfoField::new("Type", track.kind.display_name()),
                    InfoField::new("Codec", &track.codec),
                    InfoField::new("Stream ID", track.index.to_string()),
                ];
                if let Some(description) = &track.codec_long_name {
                    fields.push(InfoField::new("Codec description", description));
                }
                if let Some(language) = &track.language {
                    fields.push(InfoField::new("Language", language));
                }
                if let Some(name) = &track.name {
                    fields.push(InfoField::new("Name", name));
                }
                if !track.properties.is_empty() {
                    fields.push(InfoField::new("Properties", track.properties.join(", ")));
                }
                self.render_fields(&fields);
            }
        }
    }

    fn render_fields(&self, fields: &[InfoField]) {
        while let Some(child) = self.grid.first_child() {
            self.grid.remove(&child);
        }
        for (row, field) in fields.iter().enumerate() {
            let label = gtk::Label::new(Some(&field.label));
            label.set_xalign(0.0);
            label.set_yalign(0.0);
            label.add_css_class("info-label");
            let value = gtk::Label::new(Some(&field.value));
            value.set_xalign(0.0);
            value.set_yalign(0.0);
            value.set_wrap(true);
            value.set_selectable(true);
            value.set_hexpand(true);
            value.add_css_class("info-value");
            self.grid.attach(&label, 0, row as i32, 1, 1);
            self.grid.attach(&value, 1, row as i32, 1, 1);
        }
    }
}

fn format_playlist_ranges(key: &str) -> Option<String> {
    key.split_terminator(';')
        .map(|item| {
            let mut fields = item.split(':');
            let clip_id = fields.next()?;
            let in_time = u64::from_str_radix(fields.next()?, 16).ok()?;
            let out_time = u64::from_str_radix(fields.next()?, 16).ok()?;
            fields.next().is_none().then(|| {
                format!(
                    "{clip_id}.m2ts {}–{}",
                    format_duration(in_time / 90),
                    format_duration(out_time / 90)
                )
            })
        })
        .collect::<Option<Vec<_>>>()
        .filter(|items| !items.is_empty())
        .map(|items| items.join(", "))
}

#[cfg(test)]
mod tests {
    use super::format_playlist_ranges;

    #[test]
    fn formats_exact_playlist_ranges_for_diagnostics() {
        assert_eq!(
            format_playlist_ranges(
                "00001:0000000000000000:00000000134fd900;\
                 00042:0000000000015f90:000000000002bf20;"
            )
            .as_deref(),
            Some("00001.m2ts 0:00:00–1:00:00, 00042.m2ts 0:00:01–0:00:02")
        );
    }
}
