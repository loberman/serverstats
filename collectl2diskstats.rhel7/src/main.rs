/*
 * collectl2diskstats.rs - Converts collectl raw output to serverstats_grab-compatible .dat CSV files
 *
 * Copyright (C) 2024 Laurence Oberman <loberman@redhat.com>
 * Co-developed with assistance from ChatGPT (OpenAI)
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */

//! # collectl2diskstats
//!
//! Converts raw collectl text output (containing epoch markers, disk, cpu, meminfo, net)
//! into CSV lines compatible with serverstats_grab tools.
//!
//! - Emits DISK, CPU, MEM, and NET lines in correct format for .dat playback
//! - Handles large files efficiently and tracks progress
//! - Fails safe on missing/partial fields
//!
//! ## Authors
//! - Laurence Oberman <loberman@redhat.com>
//! - ChatGPT (OpenAI)

use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};

const VERSION_NUMBER: &str = "2.1.3";

fn main() -> io::Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        println!("collectl2diskstats-rhel7 Version {}", VERSION_NUMBER);
        println!("Convert collectl raw files to serverstats_grab dat files");
        eprintln!("Usage: {} <collectl-xxx.raw> <collectl-xxx.dat>", args[0]);
        std::process::exit(1);
    }
    println!("collectl2diskstats-rhel7 Version {}", VERSION_NUMBER);
    parse_collectl_raw_to_dat(&args[1], &args[2])
}

/// Converts a collectl raw log to a serverstats_grab-compatible .dat CSV file.
fn parse_collectl_raw_to_dat(raw_path: &str, out_path: &str) -> io::Result<()> {
    let wanted_disks = ["sd", "nvme", "dm-", "loop", "emcpower"];
    let infile = File::open(raw_path)?;
    let reader = BufReader::new(infile);
    let lines: Vec<String> = reader.lines().filter_map(Result::ok).collect();

    let mut out = File::create(out_path)?;
    writeln!(out, "#TYPE,ts_epoch,<fields...>")?;

    let mut i = 0;
    let mut input_line_num = 0usize;
    let mut epoch_count = 0usize;
    let progress_every = 10_000;

    while i < lines.len() {
        let line = &lines[i];
        input_line_num += 1;
        if input_line_num % progress_every == 0 {
            print!("\rProcessed {} lines...", input_line_num);
            std::io::stdout().flush().unwrap();
        }

        // Epoch block marker: >>> <epoch> <<<
        let ts_match = line.trim().strip_prefix(">>>")
            .and_then(|x| x.split_whitespace().next())
            .and_then(|x| x.parse::<f64>().ok());

        if ts_match.is_none() {
            i += 1;
            continue;
        }
        epoch_count += 1;
        let ts = ts_match.unwrap() as u64;

        let mut cpu_vals: Option<Vec<String>> = None;
        let mut procs_running = 0;
        let mut procs_blocked = 0;
        let mut meminfo = std::collections::HashMap::<String, String>::new();
        let mut net_lines: Vec<Vec<String>> = Vec::new();

        // Scan until next epoch or EOF
        i += 1;
        while i < lines.len() && !lines[i].starts_with(">>>") {
            input_line_num += 1;
            if input_line_num % progress_every == 0 {
                print!("\rProcessed {} lines...", input_line_num);
                std::io::stdout().flush().unwrap();
            }

            let l = lines[i].trim();

            // Skip per-CPU
            if l.starts_with("cpu") && !l.starts_with("cpu ") {
                // per-cpu, ignore
            } else if l.starts_with("cpu ") {
                cpu_vals = Some(l.split_whitespace().map(|s| s.to_string()).collect());
            } else if l.starts_with("procs_running") {
                procs_running = l.split_whitespace().nth(1).and_then(|v| v.parse().ok()).unwrap_or(0);
            } else if l.starts_with("procs_blocked") {
                procs_blocked = l.split_whitespace().nth(1).and_then(|v| v.parse().ok()).unwrap_or(0);
            } else if l.starts_with("disk") {
                let disk_fields: Vec<&str> = l.split_whitespace().collect();

                //
                // RHEL7 collectl disk format = 15 fields
                //
                // Fields:
                // 0: disk
                // 1: major
                // 2: minor
                // 3: name
                // 4–14: classic diskstats fields (no discards)
                //
                // RHEL8/9 or modern collectl = 18 fields
                //
                // Accept BOTH formats.
                //
                if disk_fields.len() == 15 || disk_fields.len() == 18 {
                    let devname = disk_fields[3];

                    if wanted_disks.iter().any(|prefix| devname.starts_with(prefix)) {
                     // Extract common 15 fields
                        let major               = disk_fields[1];
                        let minor               = disk_fields[2];
                        let name                = disk_fields[3];
                        let reads               = disk_fields[4];
                        let reads_merged        = disk_fields[5];
                        let sectors_read        = disk_fields[6];
                        let read_time_ms        = disk_fields[7];
                        let writes              = disk_fields[8];
                        let writes_merged       = disk_fields[9];
                        let sectors_written     = disk_fields[10];
                        let write_time_ms       = disk_fields[11];
                        let io_in_progress      = disk_fields[12];
                        let io_time_ms          = disk_fields[13];
                        let weighted_io_time_ms = disk_fields[14];

                        // RHEL7 (15 fields): no discard fields — pad with zeros
                        // Newer kernels (18 fields): extract real values
                        let discards           = if disk_fields.len() > 15 { disk_fields[15] } else { "0" };
                        let discards_merged    = if disk_fields.len() > 16 { disk_fields[16] } else { "0" };
                        let sectors_discarded  = if disk_fields.len() > 17 { disk_fields[17] } else { "0" };
                        let discards_time_ms   = "0";   // kernel 5.10+ has discard_time_ms, RHEL7 does not

                        writeln!(
                            out,
                            "DISK,{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
                            ts,
                            major,
                            minor,
                            name,
                            reads,
                            reads_merged,
                            sectors_read,
                            read_time_ms,
                            writes,
                            writes_merged,
                            sectors_written,
                            write_time_ms,
                            io_in_progress,
                            io_time_ms,
                            weighted_io_time_ms,
                            discards,
                            discards_merged,
                            sectors_discarded,
                           discards_time_ms
                       )?;
                    }
                }

            } else if l.contains(':') && !l.starts_with("Net ") {
                let parts: Vec<&str> = l.split_whitespace().collect();
                if parts.len() >= 2 {
                    let k = parts[0].trim_end_matches(':');
                    let wanted = [
                        "MemTotal","MemFree","MemAvailable","Buffers","Cached","SwapTotal",
                        "SwapFree","Dirty","Writeback","Active(file)","Inactive(file)","Slab",
                        "KReclaimable","SReclaimable"
                    ];
                    if wanted.contains(&k) {
                        meminfo.insert(k.to_string(), parts[1].to_string());
                    }
                }
            } else if l.trim_start().starts_with("Net ") {
                // Working NET line logic
                let parts: Vec<&str> = l.split_whitespace().collect();
                if parts.len() >= 14 {
                    let iface = parts[1].trim_end_matches(':');
                    let rx_bytes   = parts[2];
                    let rx_packets = parts[3];
                    let rx_errors  = parts[4];
                    let rx_dropped = parts[5];
                    let tx_bytes   = parts[10];
                    let tx_packets = parts[11];
                    let tx_errors  = parts[12];
                    let tx_dropped = parts[13];

                    net_lines.push(vec![
                        iface.to_string(),
                        rx_bytes.to_string(),
                        tx_bytes.to_string(),
                        rx_packets.to_string(),
                        tx_packets.to_string(),
                        rx_errors.to_string(),
                        tx_errors.to_string(),
                        rx_dropped.to_string(),
                        tx_dropped.to_string(),
                    ]);
                }
            }
            i += 1;
        }

        // Write CPU line if present for epoch
        if let Some(cpu_fields) = cpu_vals {
            if cpu_fields.len() >= 10 {
                writeln!(
                    out,
                    "CPU,{},{},{},{},{},{},{},{},{},{},{},{}",
                    ts,
                    cpu_fields[1], cpu_fields[2], cpu_fields[3], cpu_fields[4], cpu_fields[5],
                    cpu_fields[6], cpu_fields[7], cpu_fields[8], cpu_fields[9],
                    procs_running, procs_blocked
                )?;
            }
        }
        // Write MEM line if present for epoch
        if !meminfo.is_empty() {
            let fields = [
                "MemTotal","MemFree","MemAvailable","Buffers","Cached","SwapTotal","SwapFree","Dirty","Writeback","Active(file)","Inactive(file)","Slab","KReclaimable","SReclaimable"
            ];
            writeln!(
                out,
                "MEM,{},{}",
                ts,
                fields.iter()
                    .map(|k| meminfo.get(*k).cloned().unwrap_or_else(|| "0".to_string()))
                    .collect::<Vec<_>>()
                    .join(",")
            )?;
        }
        // Write NET lines if present for this epoch
        for net in &net_lines {
            writeln!(out, "NET,{},{}", ts, net.join(","))?;
        }
    }
    println!("\rProcessed {} input lines ({} epochs). Done!", input_line_num, epoch_count);
    Ok(())
}

