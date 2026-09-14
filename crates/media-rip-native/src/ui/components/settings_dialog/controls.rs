use super::*;

impl SettingsForm {
    pub(super) fn toggle(
        self: &Rc<Self>,
        value: bool,
        set: fn(&mut AppSettings, bool),
    ) -> gtk::Switch {
        let control = gtk::Switch::builder()
            .active(value)
            .valign(gtk::Align::Center)
            .build();
        let form = Rc::downgrade(self);
        control.connect_active_notify(move |control| {
            if let Some(form) = form.upgrade() {
                form.update(|settings| set(settings, control.is_active()));
            }
        });
        control
    }

    pub(super) fn entry(
        self: &Rc<Self>,
        value: &str,
        set: fn(&mut AppSettings, String),
    ) -> gtk::Entry {
        let control = gtk::Entry::builder().text(value).hexpand(true).build();
        let form = Rc::downgrade(self);
        control.connect_changed(move |control| {
            if let Some(form) = form.upgrade() {
                form.update(|settings| set(settings, control.text().trim().to_owned()));
            }
        });
        control
    }

    pub(super) fn password(
        self: &Rc<Self>,
        value: &str,
        set: fn(&mut AppSettings, String),
    ) -> gtk::PasswordEntry {
        let control = gtk::PasswordEntry::builder()
            .text(value)
            .show_peek_icon(true)
            .hexpand(true)
            .build();
        let form = Rc::downgrade(self);
        control.connect_text_notify(move |control| {
            if let Some(form) = form.upgrade() {
                form.update(|settings| set(settings, control.text().trim().to_owned()));
            }
        });
        control
    }

    pub(super) fn choice(
        self: &Rc<Self>,
        value: &str,
        options: &[(&str, &str)],
        set: fn(&mut AppSettings, String),
    ) -> gtk::ComboBoxText {
        let control = gtk::ComboBoxText::new();
        for (id, label) in options {
            control.append(Some(id), &translate(label));
        }
        control.set_active_id(Some(value));
        let form = Rc::downgrade(self);
        control.connect_changed(move |control| {
            if let (Some(form), Some(value)) = (form.upgrade(), control.active_id()) {
                form.update(|settings| set(settings, value.to_string()));
            }
        });
        control
    }

    pub(super) fn number(
        self: &Rc<Self>,
        value: u32,
        set: fn(&mut AppSettings, u32),
    ) -> gtk::SpinButton {
        let control = gtk::SpinButton::with_range(0.0, 86_400.0, 1.0);
        control.set_value(value.into());
        let form = Rc::downgrade(self);
        control.connect_value_changed(move |control| {
            if let Some(form) = form.upgrade() {
                form.update(|settings| set(settings, control.value_as_int().max(0) as u32));
            }
        });
        control
    }

    pub(super) fn folder(
        self: &Rc<Self>,
        parent: &gtk::Dialog,
        value: &str,
        set: fn(&mut AppSettings, String),
    ) -> gtk::Box {
        let control = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let entry = self.entry(value, set);
        entry.set_placeholder_text(Some("No folder override"));
        let browse = gtk::Button::builder()
            .icon_name("folder-open-symbolic")
            .tooltip_text("Choose folder")
            .build();
        control.append(&entry);
        control.append(&browse);
        let parent = parent.downgrade();
        browse.connect_clicked(move |_| {
            let Some(parent) = parent.upgrade() else {
                return;
            };
            let chooser = gtk::FileChooserNative::builder()
                .title("Choose output folder")
                .transient_for(&parent)
                .modal(true)
                .action(gtk::FileChooserAction::SelectFolder)
                .accept_label("Choose")
                .cancel_label("Cancel")
                .build();
            if !entry.text().is_empty() {
                let _ = chooser
                    .set_current_folder(Some(&gtk::gio::File::for_path(entry.text().as_str())));
            }
            let entry = entry.clone();
            chooser.connect_response(move |chooser, response| {
                if response == gtk::ResponseType::Accept
                    && let Some(path) = chooser.file().and_then(|file| file.path())
                {
                    entry.set_text(&path.to_string_lossy());
                }
                chooser.destroy();
            });
            chooser.show();
        });
        control
    }
}

pub(super) struct Page {
    id: &'static str,
    title: &'static str,
    root: gtk::Box,
    sections: Vec<Section>,
}

pub(super) struct Section {
    root: gtk::Box,
    search_text: String,
    rows: Vec<(gtk::Box, String)>,
}

impl Page {
    pub fn new(id: &'static str, title: &'static str, detail: &str) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 16);
        root.add_css_class("settings-page");
        root.append(&label(title, "settings-page-title"));
        root.append(&label(detail, "settings-page-subtitle"));
        Self {
            id,
            title,
            root,
            sections: Vec::new(),
        }
    }

    pub fn section(&mut self, title: &str, detail: &str) -> &mut Section {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("settings-section");
        let header = gtk::Box::new(gtk::Orientation::Vertical, 4);
        header.add_css_class("settings-section-header");
        header.append(&label(title, "settings-section-title"));
        header.append(&label(detail, "settings-section-subtitle"));
        root.append(&header);
        self.root.append(&root);
        self.sections.push(Section {
            root,
            search_text: format!("{} {title} {detail}", self.title).to_lowercase(),
            rows: Vec::new(),
        });
        self.sections.last_mut().expect("section just inserted")
    }

    fn filter(&self, query: &str) -> bool {
        let mut page_matches = false;
        for section in &self.sections {
            let mut section_matches = false;
            for (row, text) in &section.rows {
                let matches = query
                    .split_whitespace()
                    .all(|word| text.contains(word) || section.search_text.contains(word));
                row.set_visible(matches);
                section_matches |= matches;
            }
            section.root.set_visible(section_matches);
            page_matches |= section_matches;
        }
        page_matches
    }
}

impl Section {
    pub fn row(&mut self, title: &str, detail: &str, control: &impl IsA<gtk::Widget>) -> gtk::Box {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 16);
        row.add_css_class("settings-row");
        let labels = gtk::Box::new(gtk::Orientation::Vertical, 4);
        labels.set_hexpand(true);
        let heading = label(title, "settings-row-title");
        heading.set_mnemonic_widget(Some(control));
        labels.append(&heading);
        labels.append(&label(detail, "settings-row-detail"));
        row.append(&labels);
        control.set_valign(gtk::Align::Center);
        let column = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        column.set_size_request(240, -1);
        if control.as_ref().is::<gtk::Switch>() {
            column.set_halign(gtk::Align::End);
        } else {
            control.set_hexpand(true);
        }
        column.append(control);
        row.append(&column);
        self.root.append(&row);
        self.rows
            .push((row.clone(), format!("{title} {detail}").to_lowercase()));
        row
    }
}

fn label(text: &str, class: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(&translate(text)));
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.set_max_width_chars(46);
    label.add_css_class(class);
    label
}

pub(super) fn navigation(pages: Vec<Page>, search: &gtk::SearchEntry) -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    root.set_vexpand(true);
    root.add_css_class("settings-root");
    let sidebar = gtk::Box::new(gtk::Orientation::Vertical, 4);
    sidebar.add_css_class("settings-sidebar");
    let stack = gtk::Stack::new();
    stack.set_hexpand(true);
    stack.set_vexpand(true);
    let empty = label("No settings match your search.", "settings-page-subtitle");
    stack.add_named(&empty, Some("empty"));
    let mut first: Option<gtk::ToggleButton> = None;
    let mut entries = Vec::new();
    for page in pages {
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .child(&page.root)
            .build();
        stack.add_named(&scroll, Some(page.id));
        let button = gtk::ToggleButton::with_label(&translate(page.title));
        button.add_css_class("settings-nav-button");
        if let Some(first) = &first {
            button.set_group(Some(first));
        } else {
            first = Some(button.clone());
        }
        let stack = stack.clone();
        let id = page.id;
        button.connect_toggled(move |button| {
            if button.is_active() {
                stack.set_visible_child_name(id);
            }
        });
        sidebar.append(&button);
        entries.push((page, button));
    }
    if let Some(first) = first {
        first.set_active(true);
    }
    let stack_for_search = stack.clone();
    search.connect_search_changed(move |search| {
        let query = search.text().to_lowercase();
        let mut first_match = None;
        let mut active_matches = false;
        for (page, button) in &entries {
            let matches = page.filter(&query);
            button.set_visible(matches);
            if matches {
                first_match.get_or_insert(button);
                active_matches |= button.is_active();
            }
        }
        if !active_matches {
            if let Some(button) = first_match {
                button.set_active(true);
            } else {
                stack_for_search.set_visible_child_name("empty");
            }
        } else if let Some((page, _)) = entries.iter().find(|(_, button)| button.is_active()) {
            stack_for_search.set_visible_child_name(page.id);
        }
    });
    root.append(&sidebar);
    root.append(&gtk::Separator::new(gtk::Orientation::Vertical));
    root.append(&stack);
    root
}
