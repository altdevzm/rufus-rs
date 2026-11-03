use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{
    CloseHandle, GENERIC_READ, GENERIC_WRITE, GetLastError, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDriveStringsW,
    GetVolumeInformationW, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_READ,
    FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::IO::DeviceIoControl;
use windows_sys::Win32::System::Ioctl::{FSCTL_DISMOUNT_VOLUME, FSCTL_LOCK_VOLUME};
use windows_sys::Win32::System::WindowsProgramming::{
    DRIVE_CDROM, DRIVE_FIXED, DRIVE_RAMDISK, DRIVE_REMOTE, DRIVE_REMOVABLE,
};

use crate::types::UsbDevice;

fn to_wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn from_wide(buffer: &[u16]) -> String {
    let length = buffer.iter().position(|&value| value == 0).unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..length])
}

fn canonicalize_volume_path(raw: &str) -> String {
    let cleaned = raw.trim().trim_matches('"');
    if cleaned.is_empty() {
        return String::new();
    }

    if cleaned.len() >= 2 && cleaned.as_bytes()[1] == b':' {
        let letter = cleaned.chars().next().unwrap().to_ascii_uppercase();
        return format!(r"\\.\{}:", letter);
    }

    if cleaned.starts_with(r"\\.\") {
        return cleaned.to_string();
    }

    if cleaned.starts_with(r"\\?\") {
        return format!(r"\\.\{}", &cleaned[4..]);
    }

    cleaned.to_string()
}

fn lock_and_dismount_volume(path: &str) -> Result<(), String> {
    let target = canonicalize_volume_path(path);
    if target.is_empty() {
        return Err("Target volume path is empty".to_string());
    }

    let target_wide = to_wide(&target);
    let handle = unsafe {
        CreateFileW(
            target_wide.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null_mut(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL | FILE_FLAG_BACKUP_SEMANTICS,
            null_mut(),
        )
    };

    if handle == INVALID_HANDLE_VALUE {
        return Err(format!(
            "Could not open '{}': Windows error {}",
            target,
            unsafe { GetLastError() }
        ));
    }

    let mut bytes_returned = 0u32;
    let lock_ok = unsafe {
        DeviceIoControl(
            handle,
            FSCTL_LOCK_VOLUME,
            null_mut(),
            0,
            null_mut(),
            0,
            &mut bytes_returned,
            null_mut(),
        )
    } != 0;

    if !lock_ok {
        unsafe { CloseHandle(handle) };
        return Err(format!(
            "Could not lock '{}': Windows error {}",
            target,
            unsafe { GetLastError() }
        ));
    }

    let dismount_ok = unsafe {
        DeviceIoControl(
            handle,
            FSCTL_DISMOUNT_VOLUME,
            null_mut(),
            0,
            null_mut(),
            0,
            &mut bytes_returned,
            null_mut(),
        )
    } != 0;

    unsafe { CloseHandle(handle) };

    if !dismount_ok {
        return Err(format!(
            "Could not dismount '{}': Windows error {}",
            target,
            unsafe { GetLastError() }
        ));
    }

    Ok(())
}

pub fn list_devices() -> Vec<UsbDevice> {
    let mut buffer = vec![0u16; 512];
    let length = unsafe { GetLogicalDriveStringsW(buffer.len() as u32, buffer.as_mut_ptr()) };
    if length == 0 || length as usize >= buffer.len() {
        return Vec::new();
    }

    let system_drive = std::env::var("SystemDrive")
        .unwrap_or_else(|_| "C:".to_string())
        .to_ascii_uppercase();
    let mut devices = Vec::new();
    let mut offset = 0usize;

    while offset < length as usize {
        let end = buffer[offset..]
            .iter()
            .position(|&value| value == 0)
            .map(|index| offset + index)
            .unwrap_or(length as usize);
        if end == offset {
            break;
        }

        let drive = from_wide(&buffer[offset..=end]);
        offset = end + 1;

        let drive_letter = drive.trim_end_matches(['\\', '/']);
        let drive_wide = to_wide(&drive);
        let drive_type = unsafe { GetDriveTypeW(drive_wide.as_ptr()) };
        if matches!(drive_type, DRIVE_REMOTE | DRIVE_CDROM | DRIVE_RAMDISK)
            || !matches!(drive_type, DRIVE_FIXED | DRIVE_REMOVABLE)
        {
            continue;
        }

        let mut free_bytes = 0u64;
        let mut total_bytes = 0u64;
        let mut available_bytes = 0u64;
        let has_size = unsafe {
            GetDiskFreeSpaceExW(
                drive_wide.as_ptr(),
                &mut available_bytes,
                &mut total_bytes,
                &mut free_bytes,
            ) != 0
        };
        if !has_size || total_bytes == 0 {
            continue;
        }

        let mut label = vec![0u16; 256];
        let mut filesystem = vec![0u16; 256];
        let mut serial = 0u32;
        let mut max_component_length = 0u32;
        let mut flags = 0u32;
        let has_volume = unsafe {
            GetVolumeInformationW(
                drive_wide.as_ptr(),
                label.as_mut_ptr(),
                label.len() as u32,
                &mut serial,
                &mut max_component_length,
                &mut flags,
                filesystem.as_mut_ptr(),
                filesystem.len() as u32,
            ) != 0
        };

        let volume_label = if has_volume {
            from_wide(&label)
        } else {
            String::new()
        };
        let name = if volume_label.is_empty() {
            format!("Local volume {}", drive_letter)
        } else {
            format!("{} ({})", volume_label, drive_letter)
        };

        devices.push(UsbDevice {
            path: canonicalize_volume_path(drive_letter),
            name,
            size_bytes: total_bytes,
            is_removable: drive_type == DRIVE_REMOVABLE,
            is_system_disk: drive_letter.to_ascii_uppercase() == system_drive,
            mount_points: vec![drive_letter.to_string()],
        });
    }

    devices.sort_by(|left, right| {
        left.is_system_disk
            .cmp(&right.is_system_disk)
            .then(right.is_removable.cmp(&left.is_removable))
            .then(left.path.cmp(&right.path))
    });
    devices
}

pub fn unmount_device_partitions(dev_path: &str) -> Result<(), String> {
    lock_and_dismount_volume(dev_path)
}

#[cfg(test)]
mod tests {
    use super::{canonicalize_volume_path, from_wide, to_wide};

    #[test]
    fn test_windows_string_roundtrip() {
        let wide = to_wide(r"\\.\E:");
        assert_eq!(from_wide(&wide), r"\\.\E:");
    }

    #[test]
    fn test_volume_path_canonicalization() {
        assert_eq!(canonicalize_volume_path("E:"), r"\\.\E:");
        assert_eq!(canonicalize_volume_path(r"\\.\E:"), r"\\.\E:");
    }
}
