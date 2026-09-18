//! Detección de hardware disponible para inferencia local.

use sysinfo::System;

/// RAM que dejamos al sistema operativo y apps del usuario.
/// Recomendar un modelo que use el 100% de la RAM es recomendar una
/// máquina inutilizable.
const OS_RAM_HEADROOM: f64 = 0.70;

/// Tipo de disco — relevante en Fase 2 (expert streaming necesita NVMe).
/// La detección fiable por plataforma queda pendiente; por ahora Unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskKind {
    Nvme,
    Sata,
    Hdd,
    Unknown,
}

/// Perfil de hardware detectado en la máquina actual.
#[derive(Debug, Clone)]
pub struct HardwareProfile {
    /// Ej. "macOS", "Ubuntu 24.04"
    pub os: String,
    /// Ej. "aarch64", "x86_64"
    pub arch: String,
    pub cpu_brand: String,
    pub cpu_cores: usize,
    /// RAM total instalada en GB.
    pub total_ram_gb: f64,
    /// RAM que podemos ofrecer al modelo (total × headroom).
    pub usable_ram_gb: f64,
    /// VRAM dedicada — None en MVP (Fase 2: detección real de GPU).
    pub gpu_vram_gb: Option<f64>,
    pub disk: DiskKind,
}

/// Detecta el hardware de esta máquina.
pub fn detect_hardware() -> HardwareProfile {
    let mut sys = System::new();
    sys.refresh_cpu_all();
    sys.refresh_memory();

    let total_ram_gb = sys.total_memory() as f64 / 1_073_741_824.0;
    let cpu_brand = sys
        .cpus()
        .first()
        .map(|c| c.brand().to_string())
        .unwrap_or_else(|| "desconocido".to_string());

    HardwareProfile {
        os: System::long_os_version().unwrap_or_else(|| "desconocido".to_string()),
        arch: System::cpu_arch(),
        cpu_brand,
        cpu_cores: sys.physical_core_count().unwrap_or(sys.cpus().len()),
        total_ram_gb,
        usable_ram_gb: total_ram_gb * OS_RAM_HEADROOM,
        gpu_vram_gb: None,
        disk: DiskKind::Unknown,
    }
}
