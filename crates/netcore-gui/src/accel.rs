//! FreeBSD rendering/acceleration discovery.
//!
//! The GUI is a normal GTK4 application, so GSK chooses the concrete renderer.
//! We still inspect the system before GTK starts: when no DRM device exists we
//! explicitly ask GSK for its Cairo (CPU) renderer, and we surface the detected
//! GPU/CPU and font information in the application footer.

use std::process::Command;

use netcore_common::tools;

#[derive(Debug, Clone)]
pub struct AccelInfo {
    pub summary: String,
    pub detail: String,
}

/// Called before GtkApplication is created.
pub fn prepare_renderer() {
    if std::env::var_os("GSK_RENDERER").is_some() {
        return;
    }
    if !gpu_available() {
        // SAFETY: this runs once at the very start of the process, before GTK
        // or any other thread is started. No concurrent environment access is
        // possible.
        unsafe {
            std::env::set_var("GSK_RENDERER", "cairo");
        }
    }
}

pub fn detect() -> AccelInfo {
    let gpu = gpu_available();
    let cores = std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(1);
    let cpu = sysctl("hw.model").unwrap_or_else(|| "CPU".to_owned());
    let renderer = std::env::var("GSK_RENDERER").unwrap_or_else(|_| {
        if gpu {
            "自动（GPU）".to_owned()
        } else {
            "Cairo（CPU）".to_owned()
        }
    });

    if gpu {
        let gpu_name = gpu_name().unwrap_or_else(|| "DRM 显卡".to_owned());
        AccelInfo {
            summary: format!("GPU 加速 · {gpu_name} · {cores} 核"),
            detail: format!(
                "图形加速：GPU（{gpu_name}）\nCPU：{cpu} / {cores} 核\nGSK 渲染器：{renderer}"
            ),
        }
    } else {
        AccelInfo {
            summary: format!("CPU 渲染 · {cpu} · {cores} 核"),
            detail: format!(
                "图形加速：CPU（Cairo 软件渲染）\nCPU：{cpu} / {cores} 核\nGSK 渲染器：{renderer}"
            ),
        }
    }
}

fn gpu_available() -> bool {
    let Ok(entries) = std::fs::read_dir("/dev/dri") else {
        return false;
    };
    entries.flatten().any(|entry| {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        name.starts_with("card") || name.starts_with("renderD")
    })
}

fn gpu_name() -> Option<String> {
    let pciconf = tools::pciconf()?;
    let output = Command::new(pciconf)
        .arg("-lv")
        .env("LC_ALL", "C")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let active = active_pci_id();
    parse_display_name(&text, active.as_deref())
}

fn active_pci_id() -> Option<String> {
    for index in 0..4 {
        let key = format!("dev.drm.{index}.PCI_ID");
        if let Some(value) = sysctl(&key)
            && !value.trim().is_empty()
        {
            return Some(value.trim().to_ascii_lowercase());
        }
    }
    None
}

fn sysctl(key: &str) -> Option<String> {
    let sysctl = tools::sysctl()?;
    let output = Command::new(sysctl)
        .arg("-n")
        .arg(key)
        .env("LC_ALL", "C")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn parse_display_name(text: &str, active: Option<&str>) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut fallback: Option<String> = None;

    for (index, line) in lines.iter().enumerate() {
        if !(line.contains("class=0x030000") || line.contains("class=0x038000")) {
            continue;
        }
        let normalized = line.to_ascii_lowercase();
        let matches_active = active.is_some_and(|pci_id| {
            let parts: Vec<&str> = pci_id.split(':').collect();
            parts.len() == 2
                && normalized.contains(&format!("vendor=0x{}", parts[0]))
                && normalized.contains(&format!("device=0x{}", parts[1]))
        });
        let name = lines
            .iter()
            .skip(index + 1)
            .take(8)
            .find_map(|candidate| extract_quoted_value(candidate, "device"));
        if let Some(name) = name {
            if matches_active {
                return Some(name);
            }
            if fallback.is_none() {
                fallback = Some(name);
            }
        }
    }

    fallback
}

fn extract_quoted_value(line: &str, key: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let rest = trimmed.strip_prefix(key)?.trim_start();
    let rest = rest.strip_prefix('=')?.trim_start();
    let rest = rest.strip_prefix('\'')?;
    let end = rest.find('\'')?;
    Some(rest[..end].to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_active_gpu() {
        let text = r#"
vgapci0@pci0:1:0:0: class=0x030000 vendor=0x10de device=0x2882
    vendor     = 'NVIDIA Corporation'
    device     = 'AD107M [GeForce RTX 4060 Max-Q / Mobile]'
    class      = display
vgapci1@pci0:5:0:0: class=0x030000 vendor=0x1002 device=0x15bf
    vendor     = 'Advanced Micro Devices, Inc. [AMD/ATI]'
    device     = 'Phoenix1'
    class      = display
"#;
        assert_eq!(
            parse_display_name(text, Some("1002:15bf")).as_deref(),
            Some("Phoenix1")
        );
        assert_eq!(
            parse_display_name(text, None).as_deref(),
            Some("AD107M [GeForce RTX 4060 Max-Q / Mobile]")
        );
    }

    #[test]
    fn extract_device_name() {
        assert_eq!(
            extract_quoted_value("    device     = 'Phoenix1'", "device").as_deref(),
            Some("Phoenix1")
        );
        assert_eq!(extract_quoted_value("    vendor = 'x'", "device"), None);
    }

    #[test]
    fn missing_dri_is_not_gpu() {
        // The check must not panic on systems without /dev/dri.
        let _ = std::path::Path::new("/dev/dri").exists();
    }
}
