use std::sync::{
    Mutex,
    atomic::{AtomicU64, Ordering},
};

use media_rip_core::{MediaCatalog, TrackCoverArt};
use serde::Serialize;

static NEXT_CAPTURE_ID: AtomicU64 = AtomicU64::new(1);
static ACTIVE_METADATA_CAPTURE: Mutex<Option<ActiveMetadataCapture>> = Mutex::new(None);

struct ActiveMetadataCapture {
    id: u64,
    payloads: Vec<MetadataPayload>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct MetadataPayload {
    pub provider: String,
    pub operation: String,
    pub source: String,
    pub payload: serde_json::Value,
}

pub(crate) struct MetadataCapture {
    id: u64,
    finished: bool,
}

pub(crate) fn begin_metadata_capture() -> MetadataCapture {
    let id = NEXT_CAPTURE_ID.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut active) = ACTIVE_METADATA_CAPTURE.lock() {
        *active = Some(ActiveMetadataCapture {
            id,
            payloads: Vec::new(),
        });
    }
    MetadataCapture {
        id,
        finished: false,
    }
}

impl MetadataCapture {
    pub(crate) fn finish(mut self) -> Vec<MetadataPayload> {
        self.finished = true;
        take_metadata_capture(self.id)
    }
}

impl Drop for MetadataCapture {
    fn drop(&mut self) {
        if !self.finished {
            let _ = take_metadata_capture(self.id);
        }
    }
}

fn take_metadata_capture(id: u64) -> Vec<MetadataPayload> {
    let Ok(mut active) = ACTIVE_METADATA_CAPTURE.lock() else {
        return Vec::new();
    };
    if active.as_ref().is_none_or(|capture| capture.id != id) {
        return Vec::new();
    }
    active
        .take()
        .map(|capture| capture.payloads)
        .unwrap_or_default()
}

fn capture_metadata_payload(
    provider: &str,
    operation: &str,
    source: &str,
    value: &serde_json::Value,
) {
    if let Ok(mut active) = ACTIVE_METADATA_CAPTURE.lock()
        && let Some(active) = active.as_mut()
    {
        active.payloads.push(MetadataPayload {
            provider: provider.to_owned(),
            operation: operation.to_owned(),
            source: source.to_owned(),
            payload: value.clone(),
        });
    }
}

/// Write a complete JSON response before it is narrowed into provider-specific
/// structs. Keeping this at the API boundary makes fields that MediaRip does
/// not currently consume visible in debug logs as well.
pub(crate) fn json(provider: &str, operation: &str, source: &str, value: &serde_json::Value) {
    capture_metadata_payload(provider, operation, source, value);
    if !log::log_enabled!(log::Level::Debug) {
        return;
    }
    log::debug!(
        target: "media_rip_engine::metadata_payload",
        "provider={provider} operation={operation} source={source}\n{}",
        pretty(value)
    );
}

/// Log raw JSON emitted by local discovery tools such as ffprobe and lsblk.
pub(crate) fn tool_json(tool: &str, context: &str, bytes: &[u8]) {
    if !log::log_enabled!(log::Level::Debug) {
        return;
    }
    match serde_json::from_slice::<serde_json::Value>(bytes) {
        Ok(value) => log::debug!(
            target: "media_rip_engine::disc_payload",
            "tool={tool} context={context}\n{}",
            pretty(&value)
        ),
        Err(_) => log::debug!(
            target: "media_rip_engine::disc_payload",
            "tool={tool} context={context} raw_output={}",
            String::from_utf8_lossy(bytes)
        ),
    }
}

pub(crate) fn disc_json(source: &str, context: &str, value: &serde_json::Value) {
    if log::log_enabled!(log::Level::Debug) {
        log::debug!(
            target: "media_rip_engine::disc_payload",
            "source={source} context={context}\n{}",
            pretty(value)
        );
    }
}

/// Emit one correlated, machine-readable account of episode metadata inputs
/// and matching outcomes. This has its own target so developers can isolate it
/// from raw HTTP and disc-tool payloads.
pub(crate) fn episode_matching_report(value: &serde_json::Value) {
    if log::log_enabled!(log::Level::Debug) {
        log::debug!(
            target: "media_rip_engine::episode_matching_report",
            "correlated provider episodes and disc-title outcomes\n{}",
            pretty(value)
        );
    }
}

pub(crate) fn text(tool: &str, context: &str, value: &str) {
    if log::log_enabled!(log::Level::Debug) {
        log::debug!(
            target: "media_rip_engine::disc_payload",
            "tool={tool} context={context}\n{value}"
        );
    }
}

/// The catalog contains artwork bytes, which are binary rather than metadata.
/// Replace them with lossless descriptors so a debug scan remains readable
/// while all disk/title/track fields are still exported.
pub(crate) fn catalog(catalog: &MediaCatalog) {
    if !log::log_enabled!(log::Level::Debug) {
        return;
    }
    let track_artwork = catalog
        .track_cover_art
        .iter()
        .map(|(title_id, artwork)| {
            let description = match artwork {
                TrackCoverArt::Removed => serde_json::json!({ "state": "removed" }),
                TrackCoverArt::Custom(artwork) => serde_json::json!({
                    "state": "custom",
                    "mime_type": artwork.mime_type,
                    "byte_length": artwork.data.len(),
                }),
            };
            (title_id.to_string(), description)
        })
        .collect::<serde_json::Map<_, _>>();
    let snapshot = serde_json::json!({
        "source": catalog.source,
        "titles": catalog.titles,
        "source_info": catalog.source_info,
        "cover_art": catalog.cover_art.as_ref().map(|artwork| serde_json::json!({
            "mime_type": artwork.mime_type,
            "byte_length": artwork.data.len(),
        })),
        "track_cover_art": track_artwork,
        "metadata_attributions": catalog.metadata_attributions,
        "video_metadata_suggestions": catalog.video_metadata_suggestions,
    });
    log::debug!(
        target: "media_rip_engine::catalog_snapshot",
        "normalized disc, title, and track catalog\n{}",
        pretty(&snapshot)
    );
}

pub(crate) fn from_value<T: serde::de::DeserializeOwned>(
    provider: &str,
    operation: &str,
    source: &str,
    value: serde_json::Value,
) -> Result<T, String> {
    json(provider, operation, source, &value);
    serde_json::from_value(value).map_err(|error| error.to_string())
}

fn pretty(value: &serde_json::Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct UsedField {
        used: String,
    }

    #[test]
    fn raw_payload_keeps_fields_not_consumed_by_typed_models() {
        let raw = serde_json::json!({
            "used": "yes",
            "currently_ignored": { "track_detail": 42 }
        });
        let serialized = super::pretty(&raw);
        let typed: UsedField = super::from_value("test", "parse", "network", raw).unwrap();

        assert_eq!(typed.used, "yes");
        assert!(serialized.contains("currently_ignored"));
        assert!(serialized.contains("track_detail"));
    }

    #[test]
    fn metadata_capture_retains_the_original_value_before_typed_deserialization() {
        let capture = super::begin_metadata_capture();
        let raw = serde_json::json!({
            "used": "yes",
            "unknown_to_model": { "candidate": 42 }
        });

        let _: UsedField = super::from_value("test", "capture", "network", raw.clone()).unwrap();
        let captured = capture.finish();

        assert!(captured.iter().any(|payload| {
            payload.provider == "test" && payload.operation == "capture" && payload.payload == raw
        }));
    }
}
