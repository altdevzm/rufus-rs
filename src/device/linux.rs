use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Command;
use crate::types::UsbDevice;

/// Reads `/proc/mounts` and maps block devices to their mount points.
fn get_current_mounts() -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    if let Ok(content) = fs::read_to_string("/proc/mounts") {
        for line in content.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 && parts[0].starts_with("/dev/") {
                map.entry(parts[0].to_string())
                    .or_default()
                    .push(parts[1].to_string());
            }
        }
    }
    map
}

pub fn list_devices() -> Vec<UsbDevice> {
    let mut devices = Vec::new();
    let sys_block = Path::new("/sys/block");
    let mounts = get_current_mounts();

    if let Ok(entries) = fs::read_dir(sys_block) {
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let dev_name = file_name.to_string_lossy();

            // Filter out virtual, loop, and ram devices
            if dev_name.starts_with("loop")
                || dev_name.starts_with("ram")
                || dev_name.starts_with("zram")
                || dev_name.starts_with("dm-")
                || dev_name.starts_with("sr")
            {
                continue;
            }

            let dev_path = entry.path();
            let size_path = dev_path.join("size");
            let size_sectors: u64 = fs::read_to_string(&size_path)
                .ok()
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(0);

            if size_sectors == 0 {
                continue;
            }

            let size_bytes = size_sectors * 512;

            // Check removability flag
            let removable_path = dev_path.join("removable");
            let is_removable = fs::read_to_string(&removable_path)
                .map(|s| s.trim() == "1")
                .unwrap_or(false);

            // Read vendor and model
            let vendor = fs::read_to_string(dev_path.join("device/vendor"))
                .map(|s| s.trim().to_string())
                .unwrap_or_default();

            let model = fs::read_to_string(dev_path.join("device/model"))
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|_| dev_name.to_string());

            let full_name = if !vendor.is_empty() {
                format!("{} {}", vendor, model)
            } else {
                model
            };

            let block_dev_path = format!("/dev/{}", dev_name);

            // Collect mount points for this block device and all its child partitions
            let mut active_mounts = Vec::new();
            let mut is_system_disk = false;

            for (mounted_dev, mount_points) in &mounts {
                if mounted_dev == &block_dev_path || mounted_dev.starts_with(&block_dev_path) {
                    for mp in mount_points {
                        active_mounts.push(mp.clone());
                        if mp == "/" || mp == "/boot" || mp == "/home" || mp == "/usr" || mp == "/etc" {
                            is_system_disk = true;
                        }
                    }
                }
            }

            devices.push(UsbDevice {
                path: block_dev_path,
                name: full_name,
                size_bytes,
                is_removable,
                is_system_disk,
                mount_points: active_mounts,
            });
        }
    }

    // Sort: safe removable devices first, system disks last
    devices.sort_by(|a, b| {
        a.is_system_disk.cmp(&b.is_system_disk)
            .then(b.is_removable.cmp(&a.is_removable))
            .then(a.path.cmp(&b.path))
    });

    devices
}

/// Unmounts all mounted partitions of a target block device before raw block writing.
pub fn unmount_device_partitions(dev_path: &str) -> Result<(), String> {
    let mounts = get_current_mounts();
    for (mounted_dev, mount_points) in mounts {
        if mounted_dev == dev_path || mounted_dev.starts_with(dev_path) {
            for mp in mount_points {
                let status = Command::new("umount")
                    .arg(&mp)
                    .status()
                    .map_err(|e| format!("Failed to invoke umount for {}: {}", mp, e))?;

                if !status.success() {
                    // Try lazy unmount if busy
                    let lazy_status = Command::new("umount")
                        .arg("-l")
                        .arg(&mp)
                        .status()
                        .map_err(|e| format!("Failed to invoke lazy umount for {}: {}", mp, e))?;

                    if !lazy_status.success() {
                        return Err(format!("Could not unmount partition mounted at '{}'. Close any open files or file managers.", mp));
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_devices() {
        let devices = list_devices();
        println!("Detected {} storage devices", devices.len());
        for dev in &devices {
            println!("  {}", dev.display_name());
            assert!(!dev.path.is_empty());
        }
    }
}
