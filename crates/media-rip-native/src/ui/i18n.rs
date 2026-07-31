const TEXT_DOMAIN: &str = "media-rip";

/// Looks up user-visible text in MediaRip's gettext domain.
///
/// English source strings remain the fallback until a contributor supplies
/// and packages a translation catalog. The UI follows the system locale; it
/// deliberately has no application-language preference.
pub(crate) fn translate(message: &str) -> gtk::glib::GString {
    gtk::glib::dgettext(Some(TEXT_DOMAIN), message)
}
