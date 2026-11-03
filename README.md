# Rufus-RS: High-Performance Bootable USB Creator in Rust

A modern, fast, and cross-platform bootable USB creation tool built in **Rust** using **`egui` + `eframe`**, architected by reverse-engineering **Rufus 4.15** using Ghidra and REA.

---

## Architecture & Reverse-Engineered Features

### 1. Direct Sector Streaming Engine (`src/engine/raw_writer.rs`)
* **Sector-Aligned 4 MB Chunks**: Matches Rufus's raw block write architecture (`write_sectors`) ensuring fast I/O throughput.
* **Non-Blocking Real-time Monitoring**: Worker thread communicates via `crossbeam-channel` to report bytes written, real-time transfer speed (MB/s), and ETA without frame drops in the UI.
* **Safe Buffer Flush**: Implements `sync_all` / `fsync` barriers to prevent disk removal before write buffers are committed to flash cells.

### 2. Windows User Experience Customizer (`src/engine/unattend.rs`)
* Exact clean-room Rust implementation of Rufus's `FUN_140062e30` function discovered in Ghidra.
* Generates an automated `unattend.xml` answer file with configurable tweaks:
  * **Hardware Requirement Bypasses (`windowsPE`)**: Bypasses TPM 2.0, Secure Boot, and 4GB+ RAM checks via `LabConfig`.
  * **Microsoft Account / OOBE Bypass (`specialize`)**: Injects `BypassNRO` registry flag to allow offline local account setup in Windows 11.
  * **BitLocker Suppression**: Prevents automatic device encryption via `Microsoft-Windows-SecureStartup-FilterDriver`.
  * **Bloatware Stripping**: Automatically removes preinstalled Teams, Outlook, and disables Windows Copilot.
  * **Local Administrator Account Creation**: Generates sanitized local user accounts with custom password policies.

### 3. Native USB Device Discovery (`src/device/`)
* **Linux**: Discovers physical storage devices through `/sys/block/`, parsing drive size, vendor, model, and filtering out loop/ram devices.
* **Windows**: Designed for `SetupAPI` and `DeviceIoControl`.

---

## Project Structure

```
rufus-rs/
├── Cargo.toml
├── README.md
└── src/
    ├── main.rs                 # eframe application entry point & window styling
    ├── app.rs                  # egui immediate-mode GUI layout & state
    ├── types.rs                # Shared message protocols, device structs & enums
    ├── device/
    │   ├── mod.rs              # Device enumeration trait
    │   ├── linux.rs            # Sysfs/block discovery
    │   └── windows.rs          # SetupAPI / DeviceIoControl stub
    └── engine/
        ├── mod.rs
        ├── raw_writer.rs       # DD streaming engine with speed/ETA calculation
        └── unattend.rs         # Windows unattend.xml generator (reverse-engineered)
```

---

## How to Run

Because flashing raw blocks to physical drives requires direct device write permissions, execute with root/sudo or appropriate disk group permissions:

```bash
# Debug build
cargo run

# Or run directly with permissions
sudo ./target/debug/rufus-rs

# Optimized Release Build
cargo build --release
sudo ./target/release/rufus-rs
```

