use std::{cell::RefCell, io::Cursor, rc::Rc, sync::mpsc, thread, time::Duration};

use gtk::{glib, prelude::*};
use media_rip_core::{CoverArt, ScanOptions, ports::CancellationToken};
use media_rip_engine::{
    VideoSearchCandidate, VideoSearchKind, load_video_search_artwork, search_video_metadata,
};

#[derive(Clone)]
pub(crate) struct IdentificationDialog {
    pub dialog: gtk::Dialog,
    pub query: gtk::Entry,
    reload: gtk::CheckButton,
    kind: gtk::DropDown,
    results: gtk::ListBox,
    status: gtk::Label,
    candidates: Rc<RefCell<Vec<VideoSearchCandidate>>>,
    selected_url: Rc<RefCell<Option<String>>>,
    cancellation: Rc<RefCell<CancellationToken>>,
}

enum SearchUpdate {
    Results(Result<Vec<VideoSearchCandidate>, String>),
    Artwork(usize, CoverArt),
}

impl IdentificationDialog {
    pub fn new(
        parent: &gtk::ApplicationWindow,
        current_query: &str,
        initial_kind: VideoSearchKind,
        options: ScanOptions,
    ) -> Self {
        let dialog = gtk::Dialog::builder()
            .title("Change identification")
            .transient_for(parent)
            .modal(true)
            .default_width(820)
            .default_height(740)
            .build();
        dialog.add_button("Cancel", gtk::ResponseType::Cancel);
        dialog.add_button("Use selected match", gtk::ResponseType::Accept);
        dialog.set_response_sensitive(gtk::ResponseType::Accept, false);
        crate::ui::space_dialog_actions(
            &dialog,
            &[gtk::ResponseType::Cancel, gtk::ResponseType::Accept],
        );
        if let Some(button) = dialog
            .widget_for_response(gtk::ResponseType::Accept)
            .and_then(|widget| widget.downcast::<gtk::Button>().ok())
        {
            button.add_css_class("suggested-action");
        }

        let content = gtk::Box::new(gtk::Orientation::Vertical, 10);
        content.set_margin_top(14);
        content.set_margin_bottom(14);
        content.set_margin_start(16);
        content.set_margin_end(16);
        let heading = gtk::Label::new(Some("What movie or series is this?"));
        heading.set_xalign(0.0);
        heading.add_css_class("preflight-title");
        content.append(&heading);
        let guidance = gtk::Label::new(Some(
            "Choose Movie or TV, enter a title, then select the correct match. You don't need to know the year. You can also paste a TMDB, TVMaze or TVDB page.",
        ));
        guidance.set_xalign(0.0);
        guidance.set_wrap(true);
        guidance.add_css_class("row-detail");
        content.append(&guidance);
        let inputs = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let kind = gtk::DropDown::from_strings(&["Movie", "TV"]);
        kind.set_selected(u32::from(initial_kind == VideoSearchKind::Television));
        kind.set_tooltip_text(Some("Search movies or television series"));
        inputs.append(&kind);
        let query = gtk::Entry::builder()
            .text(current_query)
            .placeholder_text("Title or metadata page URL")
            .hexpand(true)
            .build();
        inputs.append(&query);
        let search = gtk::Button::with_label("Search");
        search.add_css_class("suggested-action");
        inputs.append(&search);
        content.append(&inputs);
        let status = gtk::Label::new(Some("Enter a title and select Search."));
        status.set_xalign(0.0);
        status.set_wrap(true);
        status.add_css_class("row-detail");
        content.append(&status);
        let results = gtk::ListBox::new();
        results.set_selection_mode(gtk::SelectionMode::Single);
        results.add_css_class("boxed-list");
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .vexpand(true)
            .child(&results)
            .build();
        content.append(&scroll);
        let reload = gtk::CheckButton::with_label("Reload source (rescan disc content)");
        reload.set_tooltip_text(Some("Read the disc again before updating metadata. Leave unchecked to use the titles already loaded."));
        content.append(&reload);
        let note = gtk::Label::new(Some(
            "Title, episode, tag and filename changes will be proposed for review. Existing track choices will be kept.",
        ));
        note.set_xalign(0.0);
        note.set_wrap(true);
        note.add_css_class("row-detail");
        content.append(&note);
        dialog.content_area().append(&content);

        let identification = Self {
            dialog,
            query,
            reload,
            kind,
            results,
            status,
            candidates: Rc::new(RefCell::new(Vec::new())),
            selected_url: Rc::new(RefCell::new(None)),
            cancellation: Rc::new(RefCell::new(CancellationToken::default())),
        };
        let selected_url = identification.selected_url.clone();
        let candidates = identification.candidates.clone();
        let weak_dialog = identification.dialog.downgrade();
        identification.results.connect_row_selected(move |_, row| {
            *selected_url.borrow_mut() = row.and_then(|row| {
                candidates
                    .borrow()
                    .get(row.index() as usize)
                    .map(|candidate| candidate.metadata_url.clone())
            });
            if let Some(dialog) = weak_dialog.upgrade() {
                dialog.set_response_sensitive(
                    gtk::ResponseType::Accept,
                    selected_url.borrow().is_some(),
                );
            }
        });
        let this = identification.clone();
        let search_options = options.clone();
        search.connect_clicked(move |_| this.search(search_options.clone()));
        let search_button = search.clone();
        identification
            .query
            .connect_activate(move |_| search_button.emit_clicked());
        let this = identification.clone();
        identification.query.connect_changed(move |_| this.reset());
        let this = identification.clone();
        identification
            .kind
            .connect_selected_notify(move |_| this.reset());
        let cancellation = identification.cancellation.clone();
        identification
            .dialog
            .connect_response(move |_, _| cancellation.borrow().cancel());
        let cancellation = identification.cancellation.clone();
        identification.dialog.connect_close_request(move |_| {
            cancellation.borrow().cancel();
            glib::Propagation::Proceed
        });
        identification
    }

    pub fn selected_metadata_url(&self) -> Option<String> {
        self.selected_url.borrow().clone()
    }

    pub fn reload_requested(&self) -> bool {
        self.reload.is_active()
    }

    fn reset(&self) {
        self.cancellation.borrow().cancel();
        *self.selected_url.borrow_mut() = None;
        self.dialog
            .set_response_sensitive(gtk::ResponseType::Accept, false);
        self.results.unselect_all();
        while let Some(child) = self.results.first_child() {
            self.results.remove(&child);
        }
        self.candidates.borrow_mut().clear();
        self.status.set_text("Enter a title and select Search.");
    }

    fn search(&self, options: ScanOptions) {
        self.reset();
        if !options.allow_online_access {
            self.status
                .set_text("Enable online metadata lookups in Settings before searching.");
            return;
        }
        let query = self.query.text().trim().to_owned();
        if query.starts_with("https://") || query.starts_with("http://") {
            match media_rip_engine::video_metadata_link_provider(&query) {
                Ok(_) => {
                    *self.selected_url.borrow_mut() = Some(query);
                    self.status
                        .set_text("Metadata page ready. Choose Use selected match to load it.");
                    self.dialog
                        .set_response_sensitive(gtk::ResponseType::Accept, true);
                }
                Err(error) => self.status.set_text(&error),
            }
            return;
        }
        if query.is_empty() {
            self.status.set_text("Enter a title to search for.");
            return;
        }
        self.status.set_text("Searching…");
        let kind = if self.kind.selected() == 0 {
            VideoSearchKind::Movie
        } else {
            VideoSearchKind::Television
        };
        let token = CancellationToken::default();
        *self.cancellation.borrow_mut() = token.clone();
        let worker_token = token.clone();
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let result = search_video_metadata(&query, kind, &options, &worker_token);
            let artwork_candidates = result.as_ref().ok().cloned().unwrap_or_default();
            if worker_token.is_cancelled() || sender.send(SearchUpdate::Results(result)).is_err() {
                return;
            }
            for (index, candidate) in artwork_candidates.iter().enumerate() {
                if worker_token.is_cancelled() {
                    break;
                }
                if let Some(artwork) = load_video_search_artwork(candidate, &options, &worker_token)
                    && sender.send(SearchUpdate::Artwork(index, artwork)).is_err()
                {
                    break;
                }
            }
        });
        let this = self.clone();
        let thumbnails = Rc::new(RefCell::new(Vec::<gtk::Image>::new()));
        glib::timeout_add_local(Duration::from_millis(50), move || {
            if token.is_cancelled() || !this.dialog.is_visible() {
                token.cancel();
                return glib::ControlFlow::Break;
            }
            loop {
                match receiver.try_recv() {
                    Ok(SearchUpdate::Results(Ok(candidates))) => {
                        *thumbnails.borrow_mut() = this.show_candidates(candidates);
                    }
                    Ok(SearchUpdate::Results(Err(error))) => this.status.set_text(&error),
                    Ok(SearchUpdate::Artwork(index, artwork)) => {
                        if let Some(image) = thumbnails.borrow().get(index)
                            && let Ok(pixbuf) =
                                gdk_pixbuf::Pixbuf::from_read(Cursor::new(artwork.data))
                            && let Some(pixbuf) = thumbnail(&pixbuf)
                        {
                            image.set_from_pixbuf(Some(&pixbuf));
                        }
                    }
                    Err(mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
                    Err(mpsc::TryRecvError::Disconnected) => return glib::ControlFlow::Break,
                }
            }
        });
    }

    fn show_candidates(&self, candidates: Vec<VideoSearchCandidate>) -> Vec<gtk::Image> {
        self.status.set_text(if candidates.is_empty() {
            "No matches found. Try a different title or content type."
        } else {
            "Select the correct title and year, then choose Use selected match."
        });
        *self.candidates.borrow_mut() = candidates;
        self.candidates
            .borrow()
            .iter()
            .map(|candidate| {
                let (row, image) = candidate_row(candidate);
                self.results.append(&row);
                image
            })
            .collect()
    }
}

fn thumbnail(pixbuf: &gdk_pixbuf::Pixbuf) -> Option<gdk_pixbuf::Pixbuf> {
    let scale = (144.0 / f64::from(pixbuf.width())).min(216.0 / f64::from(pixbuf.height()));
    pixbuf.scale_simple(
        (f64::from(pixbuf.width()) * scale).round().max(1.0) as i32,
        (f64::from(pixbuf.height()) * scale).round().max(1.0) as i32,
        gdk_pixbuf::InterpType::Bilinear,
    )
}

#[cfg(test)]
impl IdentificationDialog {
    pub(crate) fn test_selection(parent: &gtk::ApplicationWindow) {
        let dialog = Self::new(
            parent,
            "Dune",
            VideoSearchKind::Movie,
            ScanOptions::default(),
        );
        assert!(!dialog.reload_requested());
        dialog.reload.set_active(true);
        assert!(dialog.reload_requested());
        dialog.reload.set_active(false);
        let choices = [(841, 1984), (438631, 2021)].map(|(id, year)| VideoSearchCandidate {
            title: "Dune".to_owned(),
            year: Some(year),
            kind: VideoSearchKind::Movie,
            provider: "TMDB",
            metadata_url: format!("https://www.themoviedb.org/movie/{id}"),
            artwork_path: None,
        });
        let icons = dialog.show_candidates(choices.to_vec());
        assert_eq!(icons.len(), 2);
        assert!(icons[0].icon_name().is_some());
        assert_eq!(dialog.selected_metadata_url(), None);
        dialog
            .results
            .select_row(dialog.results.row_at_index(1).as_ref());
        assert_eq!(
            dialog.selected_metadata_url().as_deref(),
            Some("https://www.themoviedb.org/movie/438631")
        );
        assert!(
            dialog
                .dialog
                .widget_for_response(gtk::ResponseType::Accept)
                .unwrap()
                .is_sensitive()
        );
        dialog.query.set_text("Arrival");
        assert!(dialog.cancellation.borrow().is_cancelled());
        assert_eq!(dialog.selected_metadata_url(), None);
        assert!(dialog.results.first_child().is_none());
        assert!(
            !dialog
                .dialog
                .widget_for_response(gtk::ResponseType::Accept)
                .unwrap()
                .is_sensitive()
        );
        dialog.show_candidates(choices.to_vec());
        dialog
            .results
            .select_row(dialog.results.row_at_index(0).as_ref());
        dialog.kind.set_selected(1);
        assert_eq!(dialog.selected_metadata_url(), None);
        assert!(dialog.results.first_child().is_none());
        dialog.search(ScanOptions {
            allow_online_access: false,
            ..Default::default()
        });
        assert!(dialog.status.text().contains("Enable online"));
        dialog
            .query
            .set_text("https://www.themoviedb.org/movie/841-dune");
        dialog.search(ScanOptions::default());
        assert_eq!(
            dialog.selected_metadata_url().as_deref(),
            Some("https://www.themoviedb.org/movie/841-dune")
        );
        dialog.dialog.destroy();
    }
}

fn candidate_row(candidate: &VideoSearchCandidate) -> (gtk::Box, gtk::Image) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.set_margin_top(8);
    row.set_margin_bottom(8);
    row.set_margin_start(10);
    row.set_margin_end(10);
    let icon = match candidate.kind {
        VideoSearchKind::Movie => "video-x-generic-symbolic",
        VideoSearchKind::Television => "video-display-symbolic",
    };
    let image = gtk::Image::from_icon_name(icon);
    image.set_pixel_size(96);
    image.set_size_request(144, 216);
    row.append(&image);
    let labels = gtk::Box::new(gtk::Orientation::Vertical, 4);
    labels.set_valign(gtk::Align::Center);
    labels.set_hexpand(true);
    let title = gtk::Label::new(Some(&candidate.title));
    title.set_xalign(0.0);
    title.set_wrap(true);
    title.add_css_class("row-title");
    labels.append(&title);
    let year = candidate
        .year
        .map(|year| year.to_string())
        .unwrap_or_else(|| "Year unknown".to_owned());
    let kind = match candidate.kind {
        VideoSearchKind::Movie => "Movie",
        VideoSearchKind::Television => "TV series",
    };
    let detail = gtk::Label::new(Some(&format!("{year} · {kind} · {}", candidate.provider)));
    detail.set_xalign(0.0);
    detail.add_css_class("row-detail");
    labels.append(&detail);
    row.append(&labels);
    (row, image)
}
