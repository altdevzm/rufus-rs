use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use crossbeam_channel::Sender;
use crate::types::{FlasherMessage, UsbDevice};

const BUFFER_SIZE: usize = 4 * 1024 * 1024; // 4 MB aligned buffer

fn validate_target_device(target_device: &str, known_devices: &[UsbDevice]) -> Result<(), String> {
    let target = target_device.trim();
    if target.is_empty() {
        return Err("Target device path is empty".to_string());
    }

    let matches: Vec<_> = known_devices
        .iter()
        .filter(|device| {
            device.path == target
                || device.mount_points.iter().any(|mount| mount == target)
                || device.path.ends_with(target)
                || target.ends_with(&device.path)
        })
        .collect();

    if matches.is_empty() {
        return Err(format!(
            "Refusing to flash '{}': device was not found in the enumerated removable media list",
            target
        ));
    }

    if matches.iter().any(|device| device.is_system_disk) {
        return Err(format!(
            "Refusing to flash '{}': target is a system disk and must not be overwritten",
            target
        ));
    }

    if matches.iter().all(|device| !device.is_removable) {
        return Err(format!(
            "Refusing to flash '{}': target is not identified as removable media",
            target
        ));
    }

    Ok(())
}

fn validate_image_and_target(image_path: &Path, target_device: &str, known_devices: &[UsbDevice]) -> Result<(), String> {
    if !image_path.exists() {
        return Err(format!("Image '{}' does not exist", image_path.display()));
    }

    let image_size = std::fs::metadata(image_path)
        .map_err(|e| format!("Failed to stat image '{}': {}", image_path.display(), e))?
        .len();

    if image_size == 0 {
        return Err(format!("Image '{}' is empty and cannot be flashed", image_path.display()));
    }

    validate_target_device(target_device, known_devices)?;

    let target = known_devices
        .iter()
        .find(|device| {
            device.path == target_device
                || device.mount_points.iter().any(|mount| mount == target_device)
                || device.path.ends_with(target_device)
                || target_device.ends_with(&device.path)
        })
        .ok_or_else(|| format!(
            "Refusing to flash '{}': device is not present in the current removable-device list",
            target_device
        ))?;

    if image_size > target.size_bytes {
        return Err(format!(
            "Image '{}' is {} bytes, but target '{}' is only {} bytes; refusing to flash.",
            image_path.display(),
            image_size,
            target.path,
            target.size_bytes
        ));
    }

    Ok(())
}

pub fn flash_raw_image(
    image_path: &Path,
    target_device: &str,
    tx: Sender<FlasherMessage>,
    cancel_flag: Arc<AtomicBool>,
) -> Result<(), String> {
    let _ = tx.send(FlasherMessage::Status(format!("Opening image: {}", image_path.display())));

    let mut src = File::open(image_path)
        .map_err(|e| format!("Failed to open image file: {}", e))?;

    let total_bytes = src.metadata()
        .map_err(|e| format!("Failed to read image metadata: {}", e))?
        .len();

    let _ = tx.send(FlasherMessage::Status(format!("Opening target block device: {}", target_device)));

    let known_devices = crate::device::list_devices();
    if let Err(e) = validate_image_and_target(image_path, target_device, &known_devices) {
        let _ = tx.send(FlasherMessage::Error(format!("Preflight validation failed: {}", e)));
        return Err(e);
    }

    // Unmount any auto-mounted partitions before writing raw sectors
    let _ = tx.send(FlasherMessage::Status(format!("Checking and unmounting active partitions on {}...", target_device)));
    if let Err(e) = crate::device::unmount_device_partitions(target_device) {
        let _ = tx.send(FlasherMessage::Error(format!("Unmount failed: {}", e)));
        return Err(e);
    }

    let mut dst = OpenOptions::new()
        .write(true)
        .open(target_device)
        .map_err(|e| format!("Failed to open target device '{}': {}. (Root/Administrator permission required)", target_device, e))?;

    let _ = tx.send(FlasherMessage::Status("Streaming sectors to target drive...".to_string()));

    let mut buffer = vec![0u8; BUFFER_SIZE];
    let mut bytes_written: u64 = 0;
    let start_time = Instant::now();
    let mut last_progress_update = Instant::now();

    loop {
        if cancel_flag.load(Ordering::Relaxed) {
            let _ = tx.send(FlasherMessage::Error("Flashing aborted by user.".to_string()));
            return Err("Aborted by user".to_string());
        }

        let n = src.read(&mut buffer)
            .map_err(|e| format!("Read error: {}", e))?;

        if n == 0 {
            break;
        }

        dst.write_all(&buffer[..n])
            .map_err(|e| format!("Write error at offset {}: {}", bytes_written, e))?;

        bytes_written += n as u64;

        if last_progress_update.elapsed().as_millis() >= 200 || bytes_written == total_bytes {
            let elapsed_secs = start_time.elapsed().as_secs_f64();
            let speed_mbps = if elapsed_secs > 0.0 {
                (bytes_written as f64 / 1_048_576.0) / elapsed_secs
            } else {
                0.0
            };

            let remaining_bytes = total_bytes.saturating_sub(bytes_written);
            let eta_seconds = if speed_mbps > 0.0 {
                (remaining_bytes as f64 / (speed_mbps * 1_048_576.0)) as u64
            } else {
                0
            };

            let _ = tx.send(FlasherMessage::Progress {
                bytes_written,
                total_bytes,
                speed_mbps,
                eta_seconds,
            });

            last_progress_update = Instant::now();
        }
    }

    let _ = tx.send(FlasherMessage::Status("Synchronizing and flushing storage buffers to flash...".to_string()));
    dst.flush().map_err(|e| format!("Failed to flush buffers: {}", e))?;
    dst.sync_all().map_err(|e| format!("Failed to sync device buffers: {}", e))?;

    let total_elapsed = start_time.elapsed().as_secs_f64();
    let avg_speed = if total_elapsed > 0.0 {
        (total_bytes as f64 / 1_048_576.0) / total_elapsed
    } else {
        0.0
    };

    let _ = tx.send(FlasherMessage::Status(format!(
        "Flashing complete! Written {} in {:.1}s (avg {:.1} MB/s)",
        humansize::format_size(total_bytes, humansize::DECIMAL),
        total_elapsed,
        avg_speed
    )));
    let _ = tx.send(FlasherMessage::Success);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_target_device;
    use crate::types::UsbDevice;

    #[test]
    fn rejects_system_drive_target() {
        let devices = vec![UsbDevice {
            path: "/dev/sda".to_string(),
            name: "System Disk".to_string(),
            size_bytes: 1_000,
            is_removable: false,
            is_system_disk: true,
            mount_points: vec!["/".to_string()],
        }];

        assert!(validate_target_device("/dev/sda", &devices).is_err());
    }

    #[test]
    fn accepts_removable_usb_target() {
        let devices = vec![UsbDevice {
            path: "/dev/sdb".to_string(),
            name: "USB Flash".to_string(),
            size_bytes: 1_000,
            is_removable: true,
            is_system_disk: false,
            mount_points: vec!["/mnt/usb".to_string()],
        }];

        assert!(validate_target_device("/dev/sdb", &devices).is_ok());
    }

    #[test]
    fn rejects_image_larger_than_target() {
        let devices = vec![UsbDevice {
            path: "/dev/sdb".to_string(),
            name: "USB Flash".to_string(),
            size_bytes: 64,
            is_removable: true,
            is_system_disk: false,
            mount_points: vec!["/mnt/usb".to_string()],
        }];

        let image = std::env::temp_dir().join("image-too-large.bin");
        std::fs::write(&image, vec![0u8; 128]).unwrap();
        let result = super::validate_image_and_target(&image, "/dev/sdb", &devices);
        let _ = std::fs::remove_file(&image);
        assert!(result.is_err());
    }
}

