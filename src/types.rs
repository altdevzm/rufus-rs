#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsbDevice {
    pub path: String,          // e.g. "/dev/sdb" on Linux or "\\\\.\\PhysicalDrive1" on Windows
    pub name: String,          // e.g. "SanDisk Cruzer Blade"
    pub size_bytes: u64,
    pub is_removable: bool,
    pub is_system_disk: bool,
    pub mount_points: Vec<String>,
}

impl UsbDevice {
    pub fn display_name(&self) -> String {
        let size_str = humansize::format_size(self.size_bytes, humansize::DECIMAL);
        if self.is_system_disk {
            format!("[SYSTEM - LOCKED] {} - {} ({})", self.name, size_str, self.path)
        } else if self.is_removable {
            format!("[USB] {} - {} ({})", self.name, size_str, self.path)
        } else {
            format!("[DISK] {} - {} ({})", self.name, size_str, self.path)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartitionScheme {
    Mbr,
    Gpt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetSystem {
    BiosOrUefi,
    UefiOnly,
}

#[derive(Debug, Clone)]
pub struct WindowsTweakOptions {
    pub bypass_tpm_secureboot_ram: bool,
    pub bypass_online_account_nro: bool,
    pub disable_bitlocker: bool,
    pub create_local_account: bool,
    pub local_account_name: String,
    pub disable_telemetry_copilot: bool,
}

impl Default for WindowsTweakOptions {
    fn default() -> Self {
        Self {
            bypass_tpm_secureboot_ram: true,
            bypass_online_account_nro: true,
            disable_bitlocker: true,
            create_local_account: true,
            local_account_name: "User".to_string(),
            disable_telemetry_copilot: true,
        }
    }
}

#[derive(Debug, Clone)]
pub enum FlasherMessage {
    Status(String),
    Progress {
        bytes_written: u64,
        total_bytes: u64,
        speed_mbps: f64,
        eta_seconds: u64,
    },
    Success,
    Error(String),
}
