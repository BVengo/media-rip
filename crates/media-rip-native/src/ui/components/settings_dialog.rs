use std::{cell::RefCell, rc::Rc};

use gtk::prelude::*;

use crate::{app::AppSettings, ui::translate};

mod controls;
mod pages;
#[cfg(test)]
mod tests;

pub(crate) struct SettingsDialog {
    pub dialog: gtk::Dialog,
    pub form: Rc<SettingsForm>,
}

type Dependency = (gtk::Widget, fn(&AppSettings) -> bool);

/// A draft belongs to the dialog. Controls update it directly; Cancel never
/// mutates application settings, and Apply only accepts a valid draft.
pub(crate) struct SettingsForm {
    initial: RefCell<AppSettings>,
    draft: RefCell<AppSettings>,
    apply: gtk::Button,
    close: gtk::Button,
    error: gtk::Label,
    dependencies: RefCell<Vec<Dependency>>,
}

impl SettingsDialog {
    pub fn new(parent: &gtk::ApplicationWindow, settings: &AppSettings) -> Self {
        let dialog = gtk::Dialog::builder()
            .title("Settings")
            .transient_for(parent)
            .modal(true)
            .default_width(980)
            .default_height(720)
            .build();
        dialog.add_css_class("settings-dialog");
        dialog.add_button("Close", gtk::ResponseType::Close);
        dialog.add_button("Apply", gtk::ResponseType::Accept);
        crate::ui::space_dialog_actions(
            &dialog,
            &[gtk::ResponseType::Close, gtk::ResponseType::Accept],
        );
        let apply = dialog
            .widget_for_response(gtk::ResponseType::Accept)
            .and_then(|widget| widget.downcast::<gtk::Button>().ok())
            .expect("Apply response has a button");
        apply.add_css_class("suggested-action");
        let close = dialog
            .widget_for_response(gtk::ResponseType::Close)
            .and_then(|widget| widget.downcast::<gtk::Button>().ok())
            .expect("Close response has a button");
        let error = gtk::Label::new(None);
        error.add_css_class("error");
        error.set_wrap(true);
        error.set_xalign(0.0);
        error.set_margin_start(16);
        error.set_margin_end(16);
        let form = Rc::new(SettingsForm {
            initial: RefCell::new(settings.clone()),
            draft: RefCell::new(settings.clone()),
            apply,
            close,
            error,
            dependencies: RefCell::new(Vec::new()),
        });
        let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
        let hint = gtk::Label::new(Some(
            "Selection, metadata and format defaults apply when opening a source. Edit the current job in its workflow.",
        ));
        hint.set_wrap(true);
        hint.set_margin_top(12);
        hint.set_margin_start(16);
        hint.set_margin_end(16);
        hint.set_xalign(0.0);
        let search = gtk::SearchEntry::builder()
            .placeholder_text("Search settings")
            .build();
        search.set_margin_start(16);
        search.set_margin_end(16);
        content.append(&hint);
        content.append(&search);
        content.append(&controls::navigation(
            pages::build(&form, &dialog, settings),
            &search,
        ));
        content.append(&form.error);
        dialog.content_area().append(&content);
        form.refresh();
        Self { dialog, form }
    }
}

impl SettingsForm {
    pub fn settings(&self) -> AppSettings {
        self.draft.borrow().clone()
    }

    pub fn mark_applied(&self) {
        self.initial.replace(self.settings());
        self.refresh();
    }

    fn update(&self, change: impl FnOnce(&mut AppSettings)) {
        change(&mut self.draft.borrow_mut());
        self.refresh();
    }

    fn refresh(&self) {
        let settings = self.draft.borrow();
        let error = settings.validate().err();
        self.error.set_text(error.as_deref().unwrap_or_default());
        self.error.set_visible(error.is_some());
        self.apply
            .set_sensitive(error.is_none() && *settings != *self.initial.borrow());
        self.close
            .set_label(&translate(if *settings == *self.initial.borrow() {
                "Close"
            } else {
                "Discard unapplied changes"
            }));
        for (widget, enabled) in self.dependencies.borrow().iter() {
            widget.set_sensitive(enabled(&settings));
        }
    }

    fn enable_when(&self, widget: &impl IsA<gtk::Widget>, enabled: fn(&AppSettings) -> bool) {
        self.dependencies
            .borrow_mut()
            .push((widget.as_ref().clone(), enabled));
    }
}
