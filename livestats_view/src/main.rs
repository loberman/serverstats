/*!
 * livestats_view: Linux Server Live CPU, MEM, NET, DISK Stats Viewer
 * -------------------------------------------------------------------
 * Copyright (C) 2025 Laurence Oberman <loberman@redhat.com>
 * License: GPL v3+
 * ChatGPT (OpenAI) assisted with design and implementation.
 */

use std::{thread::sleep, time::Duration, env, fs::File, io::{BufRead, BufReader, Read}};
use std::collections::HashMap;
use chrono::Local;

// Increment as tol evolves
const VERSION_NUMBER: &str = "2.1.3";

// ======= DISK =======
#[derive(Debug, Clone)]
struct DiskStat {
    name: String,
    reads: u64,
    writes: u64,
    sectors_read: u64,
    sectors_written: u64,
    read_time_ms: u64,
    write_time_ms: u64,
    io_time_ms: u64,
    weighted_io_time_ms: u64,
}
impl DiskStat {
    fn from_line(line: &str) -> Option<Self> {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 14 { return None; }
        Some(Self {
            name: cols[2].to_string(),
            reads: cols[3].parse().ok()?,
            writes: cols[7].parse().ok()?,
            sectors_read: cols[5].parse().ok()?,
            sectors_written: cols[9].parse().ok()?,
            read_time_ms: cols[6].parse().ok()?,
            write_time_ms: cols[10].parse().ok()?,
            io_time_ms: cols[12].parse().ok()?,
            weighted_io_time_ms: cols[13].parse().ok()?,
        })
    }
}

fn run_live_disk(interval: u64, device_filter: Option<&str>) {
    let mut prev: HashMap<String, DiskStat> = HashMap::new();
    let mut printed_header = false;
    let mut output_count = 0;

    loop {
        let mut curr: HashMap<String, DiskStat> = HashMap::new();
        if let Ok(file) = File::open("/proc/diskstats") {
            let reader = BufReader::new(file);
            for line in reader.lines().flatten() {
                if let Some(stat) = DiskStat::from_line(&line) {
                    if stat.name.starts_with("sd") || stat.name.starts_with("nvme") || stat.name.starts_with("dm-") || stat.name.starts_with("vd") 
                    || stat.name.starts_with("emcpower") || stat.name.starts_with("md") || stat.name.starts_with("drbd") {
                        if let Some(filt) = device_filter {
                            if !stat.name.contains(filt) { continue; }
                        }
                        curr.insert(stat.name.clone(), stat);
                    }
                }
            }
        }
        let now = Local::now().format("%H:%M:%S").to_string();

        if !printed_header || output_count % 40 == 0 {
            println!(
                "{:<8} {:<10} {:>10} {:>10} {:>12} {:>12} {:>8} {:>12} {:>12} {:>12} {:>12}",
                "Time", "Device", "Reads/s", "Writes/s", "rd_kB/s", "wr_kB/s",
                "Qlen", "await_rd(ms)", "await_wr(ms)", "total_kB/s", "total_iops"
            );
            printed_header = true;
        }

        for (dev, stat) in &curr {
            if let Some(prev_stat) = prev.get(dev) {
                let dt = interval as f64;

                let d_reads = stat.reads.saturating_sub(prev_stat.reads);
                let d_writes = stat.writes.saturating_sub(prev_stat.writes);
                let d_sectors_read = stat.sectors_read.saturating_sub(prev_stat.sectors_read);
                let d_sectors_written = stat.sectors_written.saturating_sub(prev_stat.sectors_written);
                let delta_io_time_ms = stat.io_time_ms.saturating_sub(prev_stat.io_time_ms);
                let delta_weighted_io_time_ms = stat.weighted_io_time_ms.saturating_sub(prev_stat.weighted_io_time_ms);

                let r_s = d_reads as f64 / dt;
                let w_s = d_writes as f64 / dt;
                let rd_kbs = d_sectors_read as f64 * 512.0 / 1024.0 / dt;
                let wr_kbs = d_sectors_written as f64 * 512.0 / 1024.0 / dt;

                let total_kb = rd_kbs + wr_kbs;
                let total_iops = r_s + w_s;

                let qlen = if delta_io_time_ms > 0 {
                    delta_weighted_io_time_ms as f64 / delta_io_time_ms as f64
                } else { 0.0 };

                let await_rd_ms = if d_reads > 0 {
                    (stat.read_time_ms.saturating_sub(prev_stat.read_time_ms)) as f64 / d_reads as f64
                } else {
                    0.0
                };
                let await_wr_ms = if d_writes > 0 {
                    (stat.write_time_ms.saturating_sub(prev_stat.write_time_ms)) as f64 / d_writes as f64
                } else {
                    0.0
                };

                println!(
                    "{:<8} {:<10} {:>10.2} {:>10.2} {:>12.2} {:>12.2} {:>8.2} {:>12.2} {:>12.2} {:>12.2} {:>12.2}",
                    now, dev, r_s, w_s, rd_kbs, wr_kbs, qlen, await_rd_ms, await_wr_ms, total_kb, total_iops
                );
                output_count += 1;
            }
        }
        prev = curr;
        sleep(Duration::from_secs(interval));
    }
}

// ======= CPU =======
fn run_live_cpu(interval: u64) {
    let mut prev_vals: Option<Vec<u64>> = None;
    let mut prev_guest: u64 = 0;
    let mut printed_header = false;
    let mut output_count = 0;

    loop {
        // Read /proc/stat
        let mut buf = String::new();
        if File::open("/proc/stat").and_then(|mut f| f.read_to_string(&mut buf)).is_err() {
            eprintln!("Failed to read /proc/stat");
            return;
        }
        let mut cpu_vals: Vec<u64> = Vec::new();
        let mut running = None;
        let mut blocked = None;
        let mut guest = 0;
        for line in buf.lines() {
            if line.starts_with("cpu ") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                cpu_vals = parts[1..=8].iter().filter_map(|v| v.parse().ok()).collect();
                guest = parts.get(9).and_then(|v| v.parse().ok()).unwrap_or(0);
            } else if line.starts_with("procs_running") {
                running = line.split_whitespace().nth(1).and_then(|v| v.parse().ok());
            } else if line.starts_with("procs_blocked") {
                blocked = line.split_whitespace().nth(1).and_then(|v| v.parse().ok());
            }
        }

        if let Some(last_vals) = prev_vals.as_ref() {
            let total_diff: u64 = cpu_vals.iter().zip(last_vals.iter()).map(|(a, b)| a - b).sum::<u64>() + (guest - prev_guest);
            if total_diff == 0 {
                prev_vals = Some(cpu_vals);
                prev_guest = guest;
                sleep(Duration::from_secs(interval));
                continue;
            }
            let factor = 100.0 / total_diff as f64;
            let user   = (cpu_vals[0] - last_vals[0]) as f64 * factor;
            let nice   = (cpu_vals[1] - last_vals[1]) as f64 * factor;
            let sys    = (cpu_vals[2] - last_vals[2]) as f64 * factor;
            let idle   = (cpu_vals[3] - last_vals[3]) as f64 * factor;
            let iowait = (cpu_vals[4] - last_vals[4]) as f64 * factor;
            let guestp = (guest - prev_guest) as f64 * factor;

            let now = Local::now().format("%H:%M:%S").to_string();
            if !printed_header || output_count % 40 == 0 {
                println!(
                    "{:<8} {:>10} {:>10} {:>10} {:>10} {:>10} {:>8} {:>8} {:>10}",
                    "Time", "User(%)", "Sys(%)", "Idle(%)", "IOWait(%)", "Nice(%)",
                    "Running", "Blocked", "Guest(%)"
                );
                printed_header = true;
            }
            println!(
                "{:<8} {:>10.2} {:>10.2} {:>10.2} {:>10.2} {:>10.2} {:>8} {:>8} {:>10.2}",
                now, user, sys, idle, iowait, nice,
                running.unwrap_or(0), blocked.unwrap_or(0), guestp
            );
            output_count += 1;
        }
        prev_vals = Some(cpu_vals);
        prev_guest = guest;
        sleep(Duration::from_secs(interval));
    }
}

// ======= MEMORY =======
fn run_live_mem(interval: u64) {
    let mut printed_header = false;
    let mut output_count = 0;

    loop {
        // Parse /proc/meminfo
        let mut values = HashMap::new();
        if let Ok(file) = File::open("/proc/meminfo") {
            let reader = BufReader::new(file);
            for line in reader.lines().flatten() {
                let mut parts = line.split_whitespace();
                if let (Some(key), Some(val)) = (parts.next(), parts.next()) {
                    values.insert(key.trim_end_matches(':').to_string(), val.parse::<u64>().unwrap_or(0));
                }
            }
        }
        let mem_total = *values.get("MemTotal").unwrap_or(&1) as f64;
        let mem_free  = *values.get("MemFree").unwrap_or(&0) as f64;
        let mem_avail = *values.get("MemAvailable").unwrap_or(&0) as f64;
        let cached    = *values.get("Cached").unwrap_or(&0) as f64;
        let used = mem_total - mem_free;
        let used_percent = if mem_total > 0.0 { used / mem_total * 100.0 } else { 0.0 };
        let avail_percent = if mem_total > 0.0 { mem_avail / mem_total * 100.0 } else { 0.0 };
        let cached_percent = if mem_total > 0.0 { cached / mem_total * 100.0 } else { 0.0 };
        let free_percent = if mem_total > 0.0 { mem_free / mem_total * 100.0 } else { 0.0 };

        let now = Local::now().format("%H:%M:%S").to_string();
        if !printed_header || output_count % 40 == 0 {
            println!(
                "{:<8} {:>10} {:>10} {:>10} {:>10} {:>10} {:>10} {:>10}",
                "Time", "Used(MB)", "Free(MB)", "%Used", "%Avail", "%Cached", "%Free", "Cached(MB)"
            );
            printed_header = true;
        }
        println!(
            "{:<8} {:>10.0} {:>10.0} {:>10.2} {:>10.2} {:>10.2} {:>10.2} {:>10.0}",
            now,
            used / 1024.0, mem_free / 1024.0, used_percent, avail_percent, cached_percent, free_percent, cached / 1024.0
        );
        output_count += 1;
        sleep(Duration::from_secs(interval));
    }
}

// ======= NETWORK =======
fn run_live_net(interval: u64) {
    let mut prev: HashMap<String, [u64; 8]> = HashMap::new();
    let mut printed_header = false;
    let mut output_count = 0;

    loop {
        let mut curr: HashMap<String, [u64; 8]> = HashMap::new();
        if let Ok(file) = File::open("/proc/net/dev") {
            let reader = BufReader::new(file);
            for line in reader.lines().flatten().skip(2) {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 17 {
                    let iface = parts[0].trim_end_matches(':').to_string();
                    let rx_bytes   = parts[1].parse().unwrap_or(0);
                    let rx_packets = parts[2].parse().unwrap_or(0);
                    let rx_errors  = parts[3].parse().unwrap_or(0);
                    let rx_dropped = parts[4].parse().unwrap_or(0);
                    let tx_bytes   = parts[9].parse().unwrap_or(0);
                    let tx_packets = parts[10].parse().unwrap_or(0);
                    let tx_errors  = parts[11].parse().unwrap_or(0);
                    let tx_dropped = parts[12].parse().unwrap_or(0);
                    curr.insert(iface, [rx_bytes, tx_bytes, rx_packets, tx_packets, rx_errors, tx_errors, rx_dropped, tx_dropped]);
                }
            }
        }
        let now = Local::now().format("%H:%M:%S").to_string();
        if !printed_header || output_count % 40 == 0 {
            println!(
                "{:<8} {:<10} {:>10} {:>10} {:>10} {:>10} {:>10} {:>10} {:>10}",
                "Time", "Iface", "rx_kB/s", "tx_kB/s", "rx_pkts", "tx_pkts", "rx_err", "tx_err", "drop"
            );
            printed_header = true;
        }
        for (iface, vals) in &curr {
            if let Some(prev_vals) = prev.get(iface) {
                let drx_bytes = vals[0].saturating_sub(prev_vals[0]);
                let dtx_bytes = vals[1].saturating_sub(prev_vals[1]);
                let drx_packets = vals[2].saturating_sub(prev_vals[2]);
                let dtx_packets = vals[3].saturating_sub(prev_vals[3]);
                let drx_errs = vals[4].saturating_sub(prev_vals[4]);
                let dtx_errs = vals[5].saturating_sub(prev_vals[5]);
                let drx_drop = vals[6].saturating_sub(prev_vals[6]);
                let dtx_drop = vals[7].saturating_sub(prev_vals[7]);
                println!(
                    "{:<8} {:<10} {:>10.2} {:>10.2} {:>10} {:>10} {:>10} {:>10} {:>10}",
                    now, iface,
                    drx_bytes as f64 / interval as f64 / 1024.0,
                    dtx_bytes as f64 / interval as f64 / 1024.0,
                    drx_packets / interval, dtx_packets / interval,
                    drx_errs / interval, dtx_errs / interval, (drx_drop + dtx_drop) / interval
                );
                output_count += 1;
            }
        }
        prev = curr;
        sleep(Duration::from_secs(interval));
    }
}

// ======= USAGE & MAIN =======
fn usage() {
    println!("serverstats_grab {}", VERSION_NUMBER);
    eprintln!("livestats_view (CPU, MEM, NET, DISK live delta viewer)");
    eprintln!("Usage:");
    eprintln!("  livestats_view -g <interval_seconds> -pC            # CPU stats");
    eprintln!("  livestats_view -g <interval_seconds> -pM            # Memory stats");
    eprintln!("  livestats_view -g <interval_seconds> -pN            # Network stats");
    eprintln!("  livestats_view -g <interval_seconds> -pD [-d DEV]   # Disk stats (optional device filter)");
    eprintln!("Example:");
    eprintln!("  livestats_view -g 1 -pD -d nvme     # Only nvme devices");
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect(); // skip program name

    let mut interval: u64 = 1;
    let mut mode: Option<String> = None;
    let mut device_filter: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "-g" {
            // "-g 1" form
            if i + 1 < args.len() {
                if let Ok(val) = args[i + 1].parse() {
                    interval = val;
                    i += 2;
                    continue;
                }
            }
            eprintln!("Error: -g must be followed by interval in seconds.");
            usage();
            std::process::exit(1);
        } else if arg.starts_with("-g") && arg.len() > 2 {
            // "-g1" form
            if let Ok(val) = arg[2..].parse() {
                interval = val;
                i += 1;
                continue;
            }
            eprintln!("Error: Invalid -gN interval.");
            usage();
            std::process::exit(1);
        } else if arg == "-pC" || arg == "-pM" || arg == "-pN" || arg == "-pD" {
            mode = Some(arg.clone());
            i += 1;
            continue;
        } else if arg == "-d" {
            if i + 1 < args.len() {
                device_filter = Some(args[i + 1].clone());
                i += 2;
                continue;
            } else {
                eprintln!("Error: -d must be followed by a device filter.");
                usage();
                std::process::exit(1);
            }
        } else {
            i += 1;
        }
    }

    if mode.is_none() {
        eprintln!("Error: You must specify one of -pC, -pM, -pN, -pD.");
        usage();
        std::process::exit(1);
    }

    match mode.unwrap().as_str() {
        "-pC" => run_live_cpu(interval),
        "-pM" => run_live_mem(interval),
        "-pN" => run_live_net(interval),
        "-pD" => run_live_disk(interval, device_filter.as_deref()),
        _ => {
            usage();
            std::process::exit(1);
        }
    }
}

