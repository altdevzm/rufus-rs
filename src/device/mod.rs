#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::{list_devices, unmount_device_partitions};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{list_devices, unmount_device_partitions};

#[cfg(not(any(target_os = "linux", windows)))]
pub fn list_devices() -> Vec<crate::types::UsbDevice> {
    Vec::new()
}

#[cfg(not(any(target_os = "linux", windows)))]
pub fn unmount_device_partitions(_dev_path: &str) -> Result<(), String> {
    Ok(())
}
