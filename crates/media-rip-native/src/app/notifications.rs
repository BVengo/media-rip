use std::time::Duration;

use super::NotificationSettings;

const NTFY_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) fn publish_ntfy(
    settings: &NotificationSettings,
    title: &str,
    message: &str,
) -> Result<(), String> {
    let endpoint = ntfy_endpoint(settings)?;
    let mut request = ureq::post(&endpoint)
        .timeout(NTFY_TIMEOUT)
        .set("Title", title)
        .set("Tags", "cd,tada");
    let access_token = settings.ntfy_access_token.trim();
    if !access_token.is_empty() {
        request = request.set("Authorization", &format!("Bearer {access_token}"));
    }

    request
        .send_string(message)
        .map(|_| ())
        .map_err(|error| format!("request failed: {error}"))
}

fn ntfy_endpoint(settings: &NotificationSettings) -> Result<String, String> {
    let server = settings.ntfy_server_url.trim().trim_end_matches('/');
    if !(server.starts_with("http://") || server.starts_with("https://")) {
        return Err("ntfy server URL must begin with http:// or https://".into());
    }

    let topic = settings.ntfy_topic.trim().trim_matches('/');
    if topic.is_empty() {
        return Err("ntfy topic is empty".into());
    }
    if topic.contains('/') {
        return Err("ntfy topic must not contain '/'".into());
    }

    Ok(format!("{server}/{topic}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_endpoint_from_server_and_topic() {
        let settings = NotificationSettings {
            ntfy_server_url: "http://192.168.1.20:8080/".into(),
            ntfy_topic: "/media-rip/".into(),
            ..NotificationSettings::default()
        };

        assert_eq!(
            ntfy_endpoint(&settings).unwrap(),
            "http://192.168.1.20:8080/media-rip"
        );
    }

    #[test]
    fn rejects_missing_scheme_and_topic() {
        let mut settings = NotificationSettings {
            ntfy_server_url: "192.168.1.20:8080".into(),
            ntfy_topic: "media-rip".into(),
            ..NotificationSettings::default()
        };
        assert!(ntfy_endpoint(&settings).unwrap_err().contains("http://"));

        settings.ntfy_server_url = "http://192.168.1.20:8080".into();
        settings.ntfy_topic.clear();
        assert!(ntfy_endpoint(&settings).unwrap_err().contains("topic"));
    }
}
