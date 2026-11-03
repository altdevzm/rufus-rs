use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Instant;
use crossbeam_channel::{unbounded, Receiver};
use eframe::egui;

use crate::device::list_devices;
use crate::engine::checksum::{compute_all_checksums, ChecksumResult};
use crate::engine::raw_writer::flash_raw_image;
use crate::types::{FlasherMessage, PartitionScheme, TargetSystem, UsbDevice, WindowsTweakOptions};

// ──────────────────────────────────────────────────────────────────────────────
// State
// ──────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum ImageOption {
    Standard,
    Extended,
}

impl std::fmt::Display for ImageOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImageOption::Standard => write!(f, "Standard Windows installation"),
            ImageOption::Extended => write!(f, "Extended Windows installation (no limits)"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum ClusterSize {
    Default4096,
    Kb8,
    Kb16,
    Kb32,
    Kb64,
}
impl std::fmt::Display for ClusterSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClusterSize::Default4096 => write!(f, "4096 bytes (Default)"),
            ClusterSize::Kb8  => write!(f, "8192 bytes"),
            ClusterSize::Kb16 => write!(f, "16384 bytes"),
            ClusterSize::Kb32 => write!(f, "32768 bytes"),
            ClusterSize::Kb64 => write!(f, "65536 bytes"),
        }
    }
}

pub struct RufusApp {
    // ── Devices ────────────────────────────────────────────────────────────────
    devices: Vec<UsbDevice>,
    selected_device_idx: Option<usize>,
    last_hotplug_check: Instant,
    list_usb_hard_drives: bool,

    // ── Image file ─────────────────────────────────────────────────────────────
    selected_image: Option<PathBuf>,
    image_size_bytes: u64,
    is_windows_iso: bool,
    image_option: ImageOption,

    // ── Checksum dialog ────────────────────────────────────────────────────────
    checksum_result: Option<ChecksumResult>,
    is_computing_checksum: bool,
    checksum_rx: Option<Receiver<Result<ChecksumResult, String>>>,
    show_checksum_window: bool,

    // ── Drive properties ───────────────────────────────────────────────────────
    partition_scheme: PartitionScheme,
    target_system: TargetSystem,
    show_advanced_drive: bool,
    add_old_bios_fixes: bool,
    persistent_partition_size_gb: f32,  // 0 = disabled (only for Linux ISOs)

    // ── Format options ─────────────────────────────────────────────────────────
    volume_label: String,
    file_system: String,
    cluster_size: ClusterSize,
    show_advanced_format: bool,
    quick_format: bool,
    create_extended_label: bool,
    check_bad_blocks: bool,

    // ── Windows tweaks dialog ──────────────────────────────────────────────────
    windows_tweaks: WindowsTweakOptions,
    show_tweaks_window: bool,

    // ── Download ISO dialog ────────────────────────────────────────────────────
    show_download_dialog: bool,
    download_version: String,
    download_release: String,
    download_edition: String,
    download_language: String,
    download_arch: String,
    download_use_browser: bool,

    // ── Flashing ───────────────────────────────────────────────────────────────
    is_flashing: bool,
    progress_ratio: f32,
    status_text: String,
    speed_text: String,
    current_file_text: String,      // bottom-left "F:\sources\install.wim (4.4 GB)"
    elapsed_secs: u64,
    flash_start_time: Option<Instant>,

    // ── Confirmation ───────────────────────────────────────────────────────────
    show_confirm_dialog: bool,

    // ── Worker channels ────────────────────────────────────────────────────────
    msg_rx: Option<Receiver<FlasherMessage>>,
    cancel_flag: Arc<AtomicBool>,

    // ── Log ────────────────────────────────────────────────────────────────────
    show_log: bool,
    logs: Vec<String>,
}

impl Default for RufusApp {
    fn default() -> Self {
        let devices = list_devices();
        let selected_device_idx = if !devices.is_empty() { Some(0) } else { None };
        Self {
            devices,
            selected_device_idx,
            last_hotplug_check: Instant::now(),
            list_usb_hard_drives: false,

            selected_image: None,
            image_size_bytes: 0,
            is_windows_iso: false,
            image_option: ImageOption::Standard,

            checksum_result: None,
            is_computing_checksum: false,
            checksum_rx: None,
            show_checksum_window: false,

            partition_scheme: PartitionScheme::Gpt,
            target_system: TargetSystem::UefiOnly,
            show_advanced_drive: false,
            add_old_bios_fixes: false,
            persistent_partition_size_gb: 0.0,

            volume_label: String::new(),
            file_system: "FAT32 (Default)".to_string(),
            cluster_size: ClusterSize::Default4096,
            show_advanced_format: false,
            quick_format: true,
            create_extended_label: true,
            check_bad_blocks: false,

            windows_tweaks: WindowsTweakOptions::default(),
            show_tweaks_window: false,

            show_download_dialog: false,
            download_version: "Windows 11".to_string(),
            download_release: "22H2 v1 (Build 22621.525 - 2022.10)".to_string(),
            download_edition: "Windows 11 Home/Pro/Edu".to_string(),
            download_language: "English International".to_string(),
            download_arch: "x64".to_string(),
            download_use_browser: false,

            is_flashing: false,
            progress_ratio: 0.0,
            status_text: "READY".to_string(),
            speed_text: String::new(),
            current_file_text: String::new(),
            elapsed_secs: 0,
            flash_start_time: None,

            show_confirm_dialog: false,
            msg_rx: None,
            cancel_flag: Arc::new(AtomicBool::new(false)),

            show_log: false,
            logs: vec!["Rufus-RS initialized.".to_string()],
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// eframe::App implementation
// ──────────────────────────────────────────────────────────────────────────────

impl eframe::App for RufusApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_background(ctx);
        self.show_main_panel(ctx);
        self.show_windows(ctx);
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Background polling
// ──────────────────────────────────────────────────────────────────────────────

impl RufusApp {
    fn poll_background(&mut self, ctx: &egui::Context) {
        // Drag-and-drop
        ctx.input(|i| {
            for file in &i.raw.dropped_files {
                if let Some(ref path) = file.path {
                    self.load_image(path.clone());
                    break;
                }
            }
        });

        // Checksum
        if let Some(ref rx) = self.checksum_rx.clone() {
            if let Ok(result) = rx.try_recv() {
                match result {
                    Ok(r)  => { self.checksum_result = Some(r); self.show_checksum_window = true; }
                    Err(e) => { self.logs.push(format!("Checksum error: {e}")); }
                }
                self.is_computing_checksum = false;
                self.checksum_rx = None;
            }
        }

        // Hotplug
        if !self.is_flashing && self.last_hotplug_check.elapsed().as_secs() >= 2 {
            self.last_hotplug_check = Instant::now();
            let new_devices = list_devices();
            if new_devices != self.devices {
                self.devices = new_devices;
                if let Some(idx) = self.selected_device_idx {
                    if idx >= self.devices.len() {
                        self.selected_device_idx = if !self.devices.is_empty() { Some(0) } else { None };
                    }
                } else if !self.devices.is_empty() {
                    self.selected_device_idx = Some(0);
                }
            }
        }

        // Flashing messages
        if let Some(ref rx) = self.msg_rx {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    FlasherMessage::Status(text) => {
                        self.status_text = text.clone();
                        self.logs.push(text);
                    }
                    FlasherMessage::Progress { bytes_written, total_bytes, speed_mbps, eta_seconds: _ } => {
                        self.progress_ratio = if total_bytes > 0 {
                            bytes_written as f32 / total_bytes as f32
                        } else { 0.0 };
                        self.status_text = format!(
                            "Copying ISO files: {:.1}%",
                            self.progress_ratio * 100.0
                        );
                        self.speed_text = format!("{speed_mbps:.1} MB/s");
                    }
                    FlasherMessage::Success => {
                        self.is_flashing = false;
                        self.progress_ratio = 1.0;
                        self.status_text = "READY".to_string();
                        self.speed_text.clear();
                        self.current_file_text.clear();
                        self.flash_start_time = None;
                    }
                    FlasherMessage::Error(err) => {
                        self.is_flashing = false;
                        self.status_text = format!("ERROR: {err}");
                        self.current_file_text.clear();
                        self.flash_start_time = None;
                        self.logs.push(format!("Error: {err}"));
                    }
                }
            }
        }

        // Elapsed timer
        if let Some(start) = self.flash_start_time {
            self.elapsed_secs = start.elapsed().as_secs();
        }

        if self.is_flashing || self.is_computing_checksum {
            ctx.request_repaint();
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Main panel – pixel-for-pixel Rufus layout
// ──────────────────────────────────────────────────────────────────────────────

impl RufusApp {
    fn show_main_panel(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            // Rufus-style light grey background
            ui.style_mut().spacing.item_spacing = egui::vec2(6.0, 4.0);

            self.section_header(ui, "Drive Properties");

            // ── Device row ────────────────────────────────────────────────────
            ui.label("Device");
            ui.horizontal(|ui| {
                let current_name = self.selected_device_idx
                    .and_then(|i| self.devices.get(i))
                    .map(|d| self.device_combo_label(d))
                    .unwrap_or_else(|| "No USB devices found".to_string());

                egui::ComboBox::from_id_salt("device_combo")
                    .width(ui.available_width() - 34.0)
                    .selected_text(current_name)
                    .show_ui(ui, |ui| {
                        for (idx, dev) in self.devices.iter().enumerate() {
                            let label = self.device_combo_label(dev);
                            let disabled = dev.is_system_disk;
                            ui.add_enabled_ui(!disabled, |ui| {
                                ui.selectable_value(&mut self.selected_device_idx, Some(idx), label);
                            });
                        }
                    });

                // Save icon button (💾 = Rufus saves settings)
                let save_btn = egui::Button::new("💾").min_size(egui::vec2(28.0, 22.0));
                if ui.add(save_btn).on_hover_text("Save drive settings").clicked() {
                    self.logs.push("Settings saved.".to_string());
                }
            });

            ui.add_space(2.0);

            // ── Boot selection row ────────────────────────────────────────────
            ui.label("Boot selection");
            ui.horizontal(|ui| {
                let img_name = self.selected_image
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "Disk or ISO image (Please select)".to_string());

                egui::ComboBox::from_id_salt("boot_sel_combo")
                    .width(ui.available_width() - 98.0)
                    .selected_text(img_name)
                    .show_ui(ui, |ui| {
                        if let Some(ref p) = self.selected_image.clone() {
                            let name = p.file_name().unwrap_or_default().to_string_lossy().to_string();
                            ui.selectable_value(&mut self.selected_image, Some(p.clone()), name);
                        }
                    });

                // ✓ checksum button
                let checksum_icon = if self.is_computing_checksum { "⏳" } else { "✓" };
                let checksum_btn = egui::Button::new(checksum_icon).min_size(egui::vec2(28.0, 22.0));
                if ui.add_enabled(
                    self.selected_image.is_some() && !self.is_computing_checksum,
                    checksum_btn,
                ).on_hover_text("Compute checksums for selected image").clicked() {
                    self.start_checksum();
                }

                // SELECT button
                if ui.button("SELECT").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("ISO / Image", &["iso", "img", "bin", "raw", "vhd"])
                        .add_filter("All files", &["*"])
                        .pick_file()
                    {
                        self.load_image(path);
                    }
                }

                // ▼ dropdown arrow (opens download dialog)
                let arrow_btn = egui::Button::new("▼").min_size(egui::vec2(20.0, 22.0));
                if ui.add(arrow_btn).on_hover_text("Download ISO image").clicked() {
                    self.show_download_dialog = true;
                }
            });

            // ── Image option (Windows ISO only) ───────────────────────────────
            if self.is_windows_iso {
                ui.add_space(2.0);
                ui.label("Image option");
                let cur = self.image_option.to_string();
                egui::ComboBox::from_id_salt("image_option_combo")
                    .width(ui.available_width())
                    .selected_text(cur)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.image_option, ImageOption::Standard, ImageOption::Standard.to_string());
                        ui.selectable_value(&mut self.image_option, ImageOption::Extended, ImageOption::Extended.to_string());
                    });
            }

            ui.add_space(2.0);

            // ── Persistent partition size (Linux ISOs only) ───────────────────
            if !self.is_windows_iso && self.selected_image.is_some() {
                ui.label("Persistent partition size");
                ui.horizontal(|ui| {
                    let slider = egui::Slider::new(&mut self.persistent_partition_size_gb, 0.0..=16.0)
                        .show_value(false);
                    ui.add(slider);
                    ui.add(egui::DragValue::new(&mut self.persistent_partition_size_gb)
                        .range(0.0..=16.0)
                        .max_decimals(0)
                        .min_decimals(0));
                    ui.label("GB");
                });
            }

            ui.add_space(2.0);

            // ── Partition scheme + Target system (side by side) ───────────────
            ui.columns(2, |cols| {
                cols[0].label("Partition scheme");
                let ps_txt = match self.partition_scheme {
                    PartitionScheme::Gpt => "GPT",
                    PartitionScheme::Mbr => "MBR",
                };
                egui::ComboBox::from_id_salt("ps_combo")
                    .width(cols[0].available_width())
                    .selected_text(ps_txt)
                    .show_ui(&mut cols[0], |ui| {
                        ui.selectable_value(&mut self.partition_scheme, PartitionScheme::Gpt, "GPT");
                        ui.selectable_value(&mut self.partition_scheme, PartitionScheme::Mbr, "MBR");
                    });

                // Target system label with ? tooltip
                cols[1].horizontal(|ui| {
                    ui.label("Target system");
                    ui.label(egui::RichText::new("?").color(egui::Color32::GRAY))
                        .on_hover_text("BIOS/UEFI compatibility of the target machine");
                });
                let ts_txt = match self.target_system {
                    TargetSystem::UefiOnly   => "UEFI (non CSM)",
                    TargetSystem::BiosOrUefi => "BIOS or UEFI",
                };
                egui::ComboBox::from_id_salt("ts_combo")
                    .width(cols[1].available_width())
                    .selected_text(ts_txt)
                    .show_ui(&mut cols[1], |ui| {
                        ui.selectable_value(&mut self.target_system, TargetSystem::UefiOnly,   "UEFI (non CSM)");
                        ui.selectable_value(&mut self.target_system, TargetSystem::BiosOrUefi, "BIOS or UEFI");
                    });
            });

            ui.add_space(2.0);

            // ── Advanced drive properties ─────────────────────────────────────
            let adv_txt = if self.show_advanced_drive {
                "▼  Hide advanced drive properties"
            } else {
                "▶  Show advanced drive properties"
            };
            if ui.small_button(adv_txt).clicked() {
                self.show_advanced_drive = !self.show_advanced_drive;
            }
            if self.show_advanced_drive {
                ui.indent("adv_drive", |ui| {
                    ui.checkbox(&mut self.list_usb_hard_drives, "List USB Hard Drives");
                    ui.checkbox(&mut self.add_old_bios_fixes, "Add fixes for old BIOSes (extra partition, align, etc.)");
                    ui.add_enabled_ui(false, |ui| {
                        let mut dummy = false;
                        ui.checkbox(&mut dummy, "Use Rufus MBR with BIOS ID");
                        egui::ComboBox::from_id_salt("bios_id_combo")
                            .width(120.0)
                            .selected_text("0x80 (Default)")
                            .show_ui(ui, |_| {});
                    });
                });
            }

            // ── Format Options section ────────────────────────────────────────
            self.section_header(ui, "Format Options");

            ui.label("Volume label");
            ui.add(egui::TextEdit::singleline(&mut self.volume_label)
                .desired_width(ui.available_width()));

            ui.add_space(2.0);

            // File system + cluster size side by side
            ui.columns(2, |cols| {
                cols[0].label("File system");
                egui::ComboBox::from_id_salt("fs_combo")
                    .width(cols[0].available_width())
                    .selected_text(&self.file_system)
                    .show_ui(&mut cols[0], |ui| {
                        ui.selectable_value(&mut self.file_system, "FAT32 (Default)".to_string(), "FAT32 (Default)");
                        ui.selectable_value(&mut self.file_system, "Large FAT32".to_string(),     "Large FAT32");
                        ui.selectable_value(&mut self.file_system, "NTFS".to_string(),            "NTFS");
                        ui.selectable_value(&mut self.file_system, "exFAT".to_string(),           "exFAT");
                    });

                cols[1].label("Cluster size");
                egui::ComboBox::from_id_salt("cs_combo")
                    .width(cols[1].available_width())
                    .selected_text(self.cluster_size.to_string())
                    .show_ui(&mut cols[1], |ui| {
                        ui.selectable_value(&mut self.cluster_size, ClusterSize::Default4096, ClusterSize::Default4096.to_string());
                        ui.selectable_value(&mut self.cluster_size, ClusterSize::Kb8,  ClusterSize::Kb8.to_string());
                        ui.selectable_value(&mut self.cluster_size, ClusterSize::Kb16, ClusterSize::Kb16.to_string());
                        ui.selectable_value(&mut self.cluster_size, ClusterSize::Kb32, ClusterSize::Kb32.to_string());
                        ui.selectable_value(&mut self.cluster_size, ClusterSize::Kb64, ClusterSize::Kb64.to_string());
                    });
            });

            ui.add_space(2.0);

            // Advanced format options
            let afmt_txt = if self.show_advanced_format {
                "▼  Hide advanced format options"
            } else {
                "▶  Show advanced format options"
            };
            if ui.small_button(afmt_txt).clicked() {
                self.show_advanced_format = !self.show_advanced_format;
            }
            if self.show_advanced_format {
                ui.indent("adv_fmt", |ui| {
                    ui.checkbox(&mut self.quick_format, "Quick format");
                    ui.checkbox(&mut self.create_extended_label, "Create extended label and icon files");
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut self.check_bad_blocks, "Check device for bad blocks");
                        if self.check_bad_blocks {
                            egui::ComboBox::from_id_salt("passes_combo")
                                .width(80.0)
                                .selected_text("1 pass")
                                .show_ui(ui, |ui| {
                                    ui.label("1 pass");
                                    ui.label("2 passes");
                                    ui.label("4 passes");
                                });
                        } else {
                            ui.add_enabled_ui(false, |ui| {
                                egui::ComboBox::from_id_salt("passes_combo_dis")
                                    .width(80.0)
                                    .selected_text("1 pass")
                                    .show_ui(ui, |_| {});
                            });
                        }
                    });
                });
            }

            // ── Status section ────────────────────────────────────────────────
            self.section_header(ui, "Status");

            // Full-width progress bar with text inside (Rufus-style)
            {
                let bar_color = if self.is_flashing {
                    egui::Color32::from_rgb(0, 176, 80)   // green
                } else if self.progress_ratio >= 1.0 {
                    egui::Color32::from_rgb(0, 140, 60)   // darker green = done
                } else {
                    egui::Color32::from_rgb(200, 200, 200) // grey = idle
                };

                let desired_size = egui::vec2(ui.available_width(), 22.0);
                let (rect, _resp) = ui.allocate_exact_size(desired_size, egui::Sense::hover());

                // Background
                ui.painter().rect_filled(rect, 2.0, egui::Color32::from_rgb(220, 220, 220));
                // Progress fill
                if self.progress_ratio > 0.0 {
                    let fill_rect = egui::Rect::from_min_size(
                        rect.min,
                        egui::vec2(rect.width() * self.progress_ratio, rect.height()),
                    );
                    ui.painter().rect_filled(fill_rect, 2.0, bar_color);
                }
                // Text overlay
                let bar_text = if self.is_flashing {
                    self.status_text.clone()
                } else {
                    self.status_text.clone()
                };
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    bar_text,
                    egui::FontId::proportional(13.0),
                    egui::Color32::BLACK,
                );
            }

            ui.add_space(4.0);

            // ── Bottom toolbar ────────────────────────────────────────────────
            let is_system = self.selected_device_idx
                .and_then(|i| self.devices.get(i))
                .map(|d| d.is_system_disk)
                .unwrap_or(false);

            let can_start = !self.is_flashing
                && self.selected_device_idx.is_some()
                && self.selected_image.is_some()
                && !is_system;

            ui.horizontal(|ui| {
                // Left icons
                if ui.button("🌐").on_hover_text("Rufus website").clicked() {}
                if ui.button("ℹ").on_hover_text("About Rufus-RS").clicked() {}
                if ui.button("⚙").on_hover_text("Settings").clicked() {}
                if ui.button("📋").on_hover_text("Show log").clicked() {
                    self.show_log = !self.show_log;
                }

                // Right: START / CLOSE / CANCEL
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.is_flashing {
                        if ui.button("CANCEL").clicked() {
                            self.cancel_flag.store(true, Ordering::Relaxed);
                        }
                        ui.add_enabled_ui(false, |ui| {
                            ui.button("START");
                        });
                    } else {
                        if ui.button("CLOSE").clicked() {
                            std::process::exit(0);
                        }
                        ui.add_enabled_ui(can_start, |ui| {
                            if ui.button("START").clicked() {
                                self.show_confirm_dialog = true;
                            }
                        });
                    }
                });
            });

            // ── Status bar (bottom strip) ─────────────────────────────────────
            ui.separator();
            ui.horizontal(|ui| {
                let n = self.devices.len();
                let left_txt = if self.is_flashing && !self.current_file_text.is_empty() {
                    self.current_file_text.clone()
                } else {
                    format!("{n} device(s) found")
                };
                ui.label(egui::RichText::new(left_txt).size(11.0));

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let h = self.elapsed_secs / 3600;
                    let m = (self.elapsed_secs % 3600) / 60;
                    let s = self.elapsed_secs % 60;
                    ui.label(egui::RichText::new(format!("{h:02}:{m:02}:{s:02}")).size(11.0));
                });
            });

            // Log panel (collapsible, at the very bottom)
            if self.show_log {
                egui::ScrollArea::vertical()
                    .max_height(80.0)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for entry in &self.logs {
                            ui.label(egui::RichText::new(entry).monospace().size(10.0));
                        }
                    });
            }
        });
    }

    // Draw a bold section header with a trailing horizontal rule (Rufus style)
    fn section_header(&self, ui: &mut egui::Ui, title: &str) {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(title).size(18.0).strong());
            // Trailing line across remaining space
            let remaining = ui.available_width();
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(remaining, 2.0),
                egui::Sense::hover(),
            );
            let line_y = rect.center().y;
            ui.painter().line_segment(
                [egui::pos2(rect.left(), line_y), egui::pos2(rect.right(), line_y)],
                egui::Stroke::new(1.5, egui::Color32::GRAY),
            );
        });
        ui.add_space(2.0);
    }

    fn device_combo_label(&self, dev: &UsbDevice) -> String {
        let size = humansize::format_size(dev.size_bytes, humansize::DECIMAL);
        if dev.is_system_disk {
            format!("{} [{}] (SYSTEM)", dev.name, size)
        } else {
            format!("{} [{}]", dev.name, size)
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Sub-windows / dialogs
// ──────────────────────────────────────────────────────────────────────────────

impl RufusApp {
    fn show_windows(&mut self, ctx: &egui::Context) {
        self.show_checksum_dialog(ctx);
        self.show_tweaks_dialog(ctx);
        self.show_download_iso_dialog(ctx);
        self.show_confirm(ctx);
    }

    // ── Checksum dialog ───────────────────────────────────────────────────────
    fn show_checksum_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_checksum_window { return; }

        let title = self.selected_image
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Checksums".to_string());

        let mut open = self.show_checksum_window;
        egui::Window::new(title)
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .min_width(380.0)
            .show(ctx, |ui| {
                egui::Grid::new("checksum_grid")
                    .num_columns(2)
                    .spacing([8.0, 6.0])
                    .show(ui, |ui| {
                        if let Some(ref r) = self.checksum_result {
                            // MD5
                            ui.label(egui::RichText::new("MD5:").strong());
                            ui.add(egui::TextEdit::multiline(&mut r.md5.clone())
                                .desired_width(310.0)
                                .desired_rows(1)
                                .font(egui::TextStyle::Monospace));
                            ui.end_row();

                            // SHA1
                            ui.label(egui::RichText::new("SHA1:").strong());
                            ui.add(egui::TextEdit::multiline(&mut r.sha1.clone())
                                .desired_width(310.0)
                                .desired_rows(1)
                                .font(egui::TextStyle::Monospace));
                            ui.end_row();

                            // SHA256
                            ui.label(egui::RichText::new("SHA256:").strong());
                            ui.add(egui::TextEdit::multiline(&mut r.sha256.clone())
                                .desired_width(310.0)
                                .desired_rows(2)
                                .font(egui::TextStyle::Monospace));
                            ui.end_row();

                            // SHA512 disabled
                            ui.label(egui::RichText::new("SHA512:").strong());
                            ui.add_enabled(false, egui::TextEdit::multiline(
                                &mut "Use <Alt>-<H> (in the main\napplication window) to enable.".to_string())
                                .desired_width(310.0)
                                .desired_rows(3)
                                .font(egui::TextStyle::Monospace));
                            ui.end_row();
                        } else if self.is_computing_checksum {
                            ui.label("Computing checksums…");
                            ui.end_row();
                        }
                    });

                ui.add_space(6.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("   OK   ").clicked() {
                        self.show_checksum_window = false;
                    }
                });
            });
        self.show_checksum_window = open;
    }

    // ── Windows User Experience dialog ────────────────────────────────────────
    fn show_tweaks_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_tweaks_window { return; }

        let mut open = self.show_tweaks_window;
        let mut ok_clicked = false;
        let mut cancel_clicked = false;

        egui::Window::new("Windows User Experience")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .min_width(420.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("?").size(36.0)
                        .color(egui::Color32::from_rgb(30, 100, 200))
                        .strong());
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new("Customize Windows installation?").strong());
                        ui.add_space(8.0);
                        ui.checkbox(
                            &mut self.windows_tweaks.bypass_tpm_secureboot_ram,
                            "Remove requirement for 4GB+ RAM, Secure Boot and TPM 2.0",
                        );
                        ui.checkbox(
                            &mut self.windows_tweaks.bypass_online_account_nro,
                            "Remove requirement for an online Microsoft account",
                        );
                        ui.checkbox(
                            &mut self.windows_tweaks.disable_telemetry_copilot,
                            "Disable data collection (Skip privacy questions)",
                        );
                        ui.checkbox(
                            &mut self.windows_tweaks.create_local_account,
                            "Set a local account using the same name as this user's",
                        );
                        // Regional options (bonus)
                        let mut regional = false;
                        ui.checkbox(
                            &mut regional,
                            "Set regional options using the same values as this user's",
                        );
                    });
                });

                ui.add_space(8.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Cancel").clicked() { cancel_clicked = true; }
                    if ui.button("   OK   ").clicked() { ok_clicked = true; }
                });
            });

        if ok_clicked || cancel_clicked {
            self.show_tweaks_window = false;
        } else {
            self.show_tweaks_window = open;
        }
    }

    // ── Download ISO dialog ───────────────────────────────────────────────────
    fn show_download_iso_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_download_dialog { return; }

        let mut open = self.show_download_dialog;
        let mut download_clicked = false;
        let mut back_clicked = false;

        egui::Window::new("✏ Download ISO Image")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .min_width(380.0)
            .show(ctx, |ui| {
                egui::Grid::new("dl_grid")
                    .num_columns(1)
                    .spacing([0.0, 8.0])
                    .show(ui, |ui| {
                        // Version
                        ui.label(egui::RichText::new("Version").strong());
                        ui.end_row();
                        egui::ComboBox::from_id_salt("dl_ver")
                            .width(ui.available_width())
                            .selected_text(&self.download_version)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.download_version, "Windows 11".to_string(), "Windows 11");
                                ui.selectable_value(&mut self.download_version, "Windows 10".to_string(), "Windows 10");
                            });
                        ui.end_row();

                        // Release
                        ui.label(egui::RichText::new("Release").strong());
                        ui.end_row();
                        egui::ComboBox::from_id_salt("dl_rel")
                            .width(ui.available_width())
                            .selected_text(&self.download_release)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.download_release,
                                    "22H2 v1 (Build 22621.525 - 2022.10)".to_string(),
                                    "22H2 v1 (Build 22621.525 - 2022.10)");
                                ui.selectable_value(&mut self.download_release,
                                    "22H2 v2 (Build 22621.2715 - 2023.12)".to_string(),
                                    "22H2 v2 (Build 22621.2715 - 2023.12)");
                            });
                        ui.end_row();

                        // Edition
                        ui.label(egui::RichText::new("Edition").strong());
                        ui.end_row();
                        egui::ComboBox::from_id_salt("dl_edi")
                            .width(ui.available_width())
                            .selected_text(&self.download_edition)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.download_edition,
                                    "Windows 11 Home/Pro/Edu".to_string(),
                                    "Windows 11 Home/Pro/Edu");
                                ui.selectable_value(&mut self.download_edition,
                                    "Windows 11 Pro".to_string(),
                                    "Windows 11 Pro");
                            });
                        ui.end_row();

                        // Language
                        ui.label(egui::RichText::new("Language").strong());
                        ui.end_row();
                        egui::ComboBox::from_id_salt("dl_lang")
                            .width(ui.available_width())
                            .selected_text(&self.download_language)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.download_language, "English International".to_string(), "English International");
                                ui.selectable_value(&mut self.download_language, "English".to_string(), "English");
                                ui.selectable_value(&mut self.download_language, "French".to_string(), "French");
                                ui.selectable_value(&mut self.download_language, "German".to_string(), "German");
                            });
                        ui.end_row();

                        // Architecture
                        ui.label(egui::RichText::new("Architecture").strong());
                        ui.end_row();
                        egui::ComboBox::from_id_salt("dl_arch")
                            .width(ui.available_width())
                            .selected_text(&self.download_arch)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.download_arch, "x64".to_string(), "x64");
                                ui.selectable_value(&mut self.download_arch, "x86".to_string(), "x86");
                                ui.selectable_value(&mut self.download_arch, "arm64".to_string(), "arm64");
                            });
                        ui.end_row();

                        ui.checkbox(&mut self.download_use_browser, "Download using a browser");
                        ui.end_row();
                    });

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let dw = (ui.available_width() - 8.0) / 2.0;
                    if ui.add_sized([dw, 28.0], egui::Button::new("Download")).clicked() {
                        download_clicked = true;
                    }
                    if ui.add_sized([dw, 28.0], egui::Button::new("Back")).clicked() {
                        back_clicked = true;
                    }
                });
            });

        if download_clicked {
            self.logs.push(format!(
                "Download requested: {} {} {} {} {}",
                self.download_version, self.download_release, self.download_edition,
                self.download_language, self.download_arch
            ));
            self.show_download_dialog = false;
        } else if back_clicked {
            self.show_download_dialog = false;
        } else {
            self.show_download_dialog = open;
        }
    }

    // ── Confirmation dialog ───────────────────────────────────────────────────
    fn show_confirm(&mut self, ctx: &egui::Context) {
        if !self.show_confirm_dialog { return; }

        let dev_name = self.selected_device_idx
            .and_then(|i| self.devices.get(i))
            .map(|d| self.device_combo_label(d))
            .unwrap_or_default();

        let target = self.selected_device_idx
            .and_then(|i| self.devices.get(i))
            .map(|d| d.path.clone())
            .unwrap_or_default();

        let image_name = self.selected_image
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "No image selected".to_string());

        let image_size = self.selected_image
            .as_ref()
            .and_then(|p| std::fs::metadata(p).ok())
            .map(|m| humansize::format_size(m.len(), humansize::DECIMAL))
            .unwrap_or_else(|| "unknown".to_string());

        egui::Window::new("WARNING!")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(egui::RichText::new(
                    "You are about to perform a destructive operation.\nALL DATA ON THE TARGET DEVICE WILL BE DESTROYED!"
                ).color(egui::Color32::RED).strong());
                ui.add_space(6.0);
                ui.label(format!("Target device: {dev_name}"));
                ui.label(format!("Device path: {target}"));
                ui.label(format!("Image: {image_name}"));
                ui.label(format!("Image size: {image_size}"));
                ui.label("This action is irreversible. Confirm only if the selected removable drive is correct.");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("   OK   ").clicked() {
                        self.show_confirm_dialog = false;
                        self.start_flashing();
                    }
                    if ui.button(" CANCEL ").clicked() {
                        self.show_confirm_dialog = false;
                    }
                });
            });
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Business logic
// ──────────────────────────────────────────────────────────────────────────────

impl RufusApp {
    fn load_image(&mut self, path: PathBuf) {
        // Detect Windows ISO (has sources/install.wim)
        self.is_windows_iso = self.detect_windows_iso(&path);

        // File size
        self.image_size_bytes = std::fs::metadata(&path)
            .map(|m| m.len())
            .unwrap_or(0);

        // Volume label from filename stem
        if let Some(stem) = path.file_stem() {
            self.volume_label = stem.to_string_lossy()
                .chars().take(11).collect::<String>().to_uppercase();
        }

        // Auto file-system
        if self.is_windows_iso {
            self.file_system = "NTFS".to_string();
            self.partition_scheme = PartitionScheme::Gpt;
            self.target_system = TargetSystem::UefiOnly;
        } else {
            self.file_system = "FAT32 (Default)".to_string();
        }

        self.logs.push(format!("Loaded image: {}", path.display()));
        self.selected_image = Some(path);
        self.checksum_result = None;
    }

    fn detect_windows_iso(&self, path: &PathBuf) -> bool {
        // Quick heuristic: filename contains "Win" or has ".iso" and contains windows markers
        let name = path.file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        name.contains("win") || name.starts_with("ccc") || name.starts_with("en_win")
    }

    fn start_checksum(&mut self) {
        let path = match self.selected_image.clone() {
            Some(p) => p,
            None => return,
        };
        self.is_computing_checksum = true;
        self.checksum_result = None;
        let (tx, rx) = unbounded::<Result<ChecksumResult, String>>();
        self.checksum_rx = Some(rx);
        thread::spawn(move || {
            let _ = tx.send(compute_all_checksums(&path));
        });
    }

    fn start_flashing(&mut self) {
        let device = match self.selected_device_idx.and_then(|i| self.devices.get(i)) {
            Some(d) => d.clone(),
            None => return,
        };
        if device.is_system_disk {
            self.logs.push("BLOCKED: attempted to flash system disk.".to_string());
            return;
        }
        let image_path = match self.selected_image.clone() {
            Some(p) => p,
            None => return,
        };

        self.is_flashing = true;
        self.progress_ratio = 0.0;
        self.status_text = "Starting…".to_string();
        self.flash_start_time = Some(Instant::now());
        self.cancel_flag.store(false, Ordering::Relaxed);

        let (tx, rx) = unbounded();
        self.msg_rx = Some(rx);
        let cancel_flag = self.cancel_flag.clone();
        let target_path = device.path.clone();

        thread::spawn(move || {
            let _ = flash_raw_image(&image_path, &target_path, tx, cancel_flag);
        });
    }
}
