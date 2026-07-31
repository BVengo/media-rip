#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
pub use linux::discover_optical_drives;

#[cfg(not(target_os = "linux"))]
pub fn discover_optical_drives()
-> Result<Vec<media_rip_core::Drive>, media_rip_core::MediaEngineError> {
    Ok(Vec::new())
}
