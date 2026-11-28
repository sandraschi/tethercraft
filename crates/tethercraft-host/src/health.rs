//! Host "all is well" health collector: CPU, RAM, NVIDIA GPU, fixed disks,
//! and watched dev/AI services (ollama, docker, opencode, antigravity, cursor).
//!
//! Runs locally (`tethercraft health --json` for DevCraft or scripts) or
//! remotely over an E2E session (`HealthReq` -> `HealthResp`). Pure safe Rust,
//! no elevated privileges required.

use std::net::{SocketAddr, TcpStream};
use std::process::Command;
use std::time::Duration;

use tethercraft_proto::{AppHealth, DiskHealth, GpuHealth, HealthReport};

pub const DEFAULT_WATCH_APPS: &[&str] = &["ollama", "docker", "opencode", "antigravity", "cursor"];

/// Collect a structured health snapshot of this machine.
pub fn collect_health(req_id: u64, watch: &[String]) -> HealthReport {
    let host = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "unknown-host".to_string());
    let os = format!("{}/{}", std::env::consts::OS, std::env::consts::ARCH);

    let (cpu_load_pct, ram_used_gb, ram_total_gb, disks) = probe_sys_and_disks();
    let gpu = probe_nvidia_gpu();
    let watch_list: Vec<String> = if watch.is_empty() {
        DEFAULT_WATCH_APPS
            .iter()
            .map(|s| (*s).to_string())
            .collect()
    } else {
        watch.to_vec()
    };
    let apps = probe_watched_apps(&watch_list);

    let mut warnings = Vec::new();
    if let Some(cpu) = cpu_load_pct {
        if cpu >= 95 {
            warnings.push(format!("cpu load high ({cpu}%)"));
        }
    }
    if let (Some(used), Some(total)) = (ram_used_gb, ram_total_gb) {
        if total > 0.0 && (used / total) >= 0.92 {
            warnings.push(format!("ram nearly full ({used:.1}/{total:.1} GB)"));
        }
    }
    if let Some(g) = &gpu {
        if g.temp_c >= 85 {
            warnings.push(format!("gpu temp high ({} C)", g.temp_c));
        }
        if g.vram_total_mib > 0 && (g.vram_used_mib * 100 / g.vram_total_mib) >= 92 {
            warnings.push(format!(
                "gpu vram high ({}/{} MiB)",
                g.vram_used_mib, g.vram_total_mib
            ));
        }
    }
    for d in &disks {
        if d.warn {
            warnings.push(format!(
                "disk {} at {}% ({:.1} GB free)",
                d.name, d.used_pct, d.free_gb
            ));
        }
    }
    for a in &apps {
        if !a.running {
            warnings.push(format!("{} not running", a.name));
        }
    }

    let status = if warnings.is_empty() {
        "ok".to_string()
    } else {
        "warn".to_string()
    };

    HealthReport {
        req_id,
        host,
        os,
        status,
        cpu_load_pct,
        ram_used_gb,
        ram_total_gb,
        gpu,
        disks,
        apps,
        warnings,
    }
}

fn probe_nvidia_gpu() -> Option<GpuHealth> {
    let out = Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,temperature.gpu,utilization.gpu,memory.used,memory.total,power.draw",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    parse_nvidia_smi_line(text.lines().next()?)
}

fn parse_nvidia_smi_line(line: &str) -> Option<GpuHealth> {
    let parts: Vec<&str> = line.split(',').map(str::trim).collect();
    if parts.len() < 6 {
        return None;
    }
    Some(GpuHealth {
        name: parts[0].to_string(),
        temp_c: parts[1].parse::<u32>().ok()?,
        util_pct: parts[2].parse::<u32>().ok()?,
        vram_used_mib: parts[3].parse::<u32>().ok()?,
        vram_total_mib: parts[4].parse::<u32>().ok()?,
        power_w: parts[5].parse::<f32>().unwrap_or(0.0),
    })
}

fn probe_sys_and_disks() -> (Option<u8>, Option<f64>, Option<f64>, Vec<DiskHealth>) {
    #[cfg(windows)]
    {
        let script = r#"
$os = Get-CimInstance Win32_OperatingSystem
$cpu = (Get-CimInstance Win32_Processor | Measure-Object -Property LoadPercentage -Average).Average
$totalGb = [math]::Round($os.TotalVisibleMemorySize / 1MB, 1)
$freeGb = [math]::Round($os.FreePhysicalMemory / 1MB, 1)
$usedGb = [math]::Round($totalGb - $freeGb, 1)
$disks = @(Get-CimInstance Win32_LogicalDisk -Filter 'DriveType=3' | ForEach-Object {
  $tot = [math]::Round($_.Size / 1GB, 1)
  $fr = [math]::Round($_.FreeSpace / 1GB, 1)
  $pct = if ($_.Size -gt 0) { [int][math]::Round((($_.Size - $_.FreeSpace) / $_.Size) * 100) } else { 0 }
  @{ name = $_.DeviceID; free_gb = $fr; total_gb = $tot; used_pct = $pct }
})
@{ cpu_pct = if ($null -ne $cpu) { [int][math]::Round($cpu) } else { $null }; ram_used_gb = $usedGb; ram_total_gb = $totalGb; disks = $disks } | ConvertTo-Json -Compress -Depth 4
"#;
        let out = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .output();
        if let Ok(o) = out {
            if o.status.success() {
                let raw = String::from_utf8_lossy(&o.stdout);
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw.trim()) {
                    let cpu = v
                        .get("cpu_pct")
                        .and_then(|x| x.as_u64())
                        .and_then(|x| u8::try_from(x.min(100)).ok());
                    let ram_used = v.get("ram_used_gb").and_then(|x| x.as_f64());
                    let ram_total = v.get("ram_total_gb").and_then(|x| x.as_f64());
                    let mut disks = Vec::new();
                    if let Some(arr) = v.get("disks").and_then(|x| x.as_array()) {
                        for item in arr {
                            let name = item
                                .get("name")
                                .and_then(|x| x.as_str())
                                .unwrap_or("")
                                .to_string();
                            let free_gb =
                                item.get("free_gb").and_then(|x| x.as_f64()).unwrap_or(0.0);
                            let total_gb =
                                item.get("total_gb").and_then(|x| x.as_f64()).unwrap_or(0.0);
                            let used_pct = item
                                .get("used_pct")
                                .and_then(|x| x.as_u64())
                                .and_then(|x| u8::try_from(x.min(100)).ok())
                                .unwrap_or(0);
                            let warn = used_pct >= 85 || (total_gb > 0.0 && free_gb < 20.0);
                            if !name.is_empty() {
                                disks.push(DiskHealth {
                                    name,
                                    free_gb,
                                    total_gb,
                                    used_pct,
                                    warn,
                                });
                            }
                        }
                    }
                    return (cpu, ram_used, ram_total, disks);
                }
            }
        }
        (None, None, None, Vec::new())
    }
    #[cfg(not(windows))]
    {
        (None, None, None, Vec::new())
    }
}

fn probe_watched_apps(watch: &[String]) -> Vec<AppHealth> {
    let procs = list_process_names();
    let mut out = Vec::with_capacity(watch.len());
    for raw_name in watch {
        let key = raw_name.trim().to_lowercase();
        if key.is_empty() {
            continue;
        }
        let patterns: Vec<&str> = match key.as_str() {
            "ollama" => vec!["ollama.exe", "ollama_llama_server.exe", "ollama"],
            "docker" => vec![
                "dockerd.exe",
                "com.docker.backend.exe",
                "docker desktop.exe",
                "dockerd",
            ],
            "opencode" => vec!["opencode.exe", "opencode"],
            "antigravity" | "agy" => vec!["antigravity.exe", "agy.exe", "antigravity"],
            "cursor" => vec!["cursor.exe", "cursor"],
            other => vec![other],
        };
        let count = count_matching_processes(&procs, &patterns);
        let mut running = count > 0;
        let mut detail = if count > 0 {
            format!("{count} proc")
        } else {
            "not running".to_string()
        };
        if key == "ollama" {
            let api_up = tcp_port_open("127.0.0.1:11434");
            if api_up {
                running = true;
                detail = format!("{count} proc, api :11434 ok");
            } else if running {
                detail = format!("{count} proc, api :11434 closed");
            }
        }
        out.push(AppHealth {
            name: key,
            running,
            processes: count,
            detail,
        });
    }
    out
}

fn tcp_port_open(addr: &str) -> bool {
    match addr.parse::<SocketAddr>() {
        Ok(sa) => TcpStream::connect_timeout(&sa, Duration::from_millis(250)).is_ok(),
        Err(_) => false,
    }
}

fn list_process_names() -> Vec<String> {
    #[cfg(windows)]
    {
        let out = Command::new("tasklist")
            .args(["/FO", "CSV", "/NH"])
            .output();
        match out {
            Ok(o) if o.status.success() => parse_tasklist_csv(&String::from_utf8_lossy(&o.stdout)),
            _ => Vec::new(),
        }
    }
    #[cfg(not(windows))]
    {
        let out = Command::new("ps").args(["-e", "-o", "comm="]).output();
        match out {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(|l| l.trim().to_lowercase())
                .filter(|l| !l.is_empty())
                .collect(),
            _ => Vec::new(),
        }
    }
}

fn parse_tasklist_csv(csv: &str) -> Vec<String> {
    csv.lines()
        .filter_map(|line| {
            let first = line.split(',').next()?.trim().trim_matches('"');
            if first.is_empty() {
                None
            } else {
                Some(first.to_lowercase())
            }
        })
        .collect()
}

fn count_matching_processes(procs: &[String], patterns: &[&str]) -> u32 {
    let mut n = 0u32;
    for p in procs {
        if patterns.iter().any(|pat| p == pat || p.contains(pat)) {
            n = n.saturating_add(1);
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nvidia_smi_csv() {
        let g = parse_nvidia_smi_line("NVIDIA GeForce RTX 4090, 42, 7, 1520, 24564, 28.4");
        assert!(g.is_some());
        if let Some(gpu) = g {
            assert!(gpu.name.contains("4090"));
            assert!(gpu.temp_c == 42);
            assert!(gpu.util_pct == 7);
            assert!(gpu.vram_used_mib == 1520);
            assert!(gpu.vram_total_mib == 24564);
        }
        assert!(parse_nvidia_smi_line("bad,line").is_none());
    }

    #[test]
    fn parses_tasklist_and_matches_apps() {
        let csv = "\"ollama.exe\",\"1234\",\"Console\",\"1\",\"50,000 K\"\n\
                   \"Cursor.exe\",\"2345\",\"Console\",\"1\",\"200,000 K\"\n\
                   \"Cursor.exe\",\"2346\",\"Console\",\"1\",\"100,000 K\"\n";
        let procs = parse_tasklist_csv(csv);
        assert!(procs.len() == 3);
        assert!(count_matching_processes(&procs, &["ollama.exe"]) == 1);
        assert!(count_matching_processes(&procs, &["cursor.exe"]) == 2);
        assert!(count_matching_processes(&procs, &["opencode.exe"]) == 0);
    }
}
