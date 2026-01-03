// procstats_grab - Process Stats Gatherer & Analyzer
// Copyright (C) 2024 Laurence Oberman
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

/*!
    # gather.rs

    Process stats gatherer for `procstats_grab` (Rust Linux process/thread gather utility).

    - Scans `/proc` for all running processes and their threads.
    - Extracts process metadata, CPU stats, memory usage, I/O counters, and command line.
    - Writes out a timestamped CSV suitable for later analysis or playback.
    - Robust to permission errors and /proc races.

    # Usage

    Call [`run_gather(interval_secs)`] to start gathering at the given interval (seconds).

    CSV file output will be named: `procstats_gather-<hostname>-<YYYYMMDD-HHMMSS>.csv`
*/

use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::thread;

extern crate libc;
extern crate chrono;

use chrono::{Datelike, Timelike, Local};
use std::thread::spawn;

#[derive(serde::Serialize)]
struct CsvRow {
    ts_epoch: u64,
    pid: u32,
    ppid: u32,
    tid: u32,
    comm: String,
    state: String,
    utime: u64,
    stime: u64,
    num_threads: Option<u32>,
    vmrss_kb: Option<u64>,
    vm_size_kb: Option<u64>,
    read_bytes: u64,
    write_bytes: u64,
    cmdline: String,
}

/// Gather process stats at a specified interval and write to a CSV file.
/// Loops forever until interrupted.
///
/// # Arguments
/// * `interval_secs` - sampling interval in seconds
pub fn run_gather(interval_secs: u64) -> std::io::Result<()> {
    let hostname = get_hostname();
    let time_str = get_time_string();
    let output_file = format!("procstats_gather-{}-{}.csv", hostname, time_str);

    let mut wtr = csv::WriterBuilder::new()
    .has_headers(false)
    .from_writer(File::create(&output_file)?);

    // CSV header (exactly 14 fields)
    wtr.write_record(&[
        "ts_epoch", "pid", "ppid", "tid", "comm", "state", "utime", "stime",
        "num_threads", "vmrss_kb", "vm_size_kb", "read_bytes", "write_bytes", "cmdline"
    ])?;

    println!(
        "procstats_grab (Rust Linux process/thread gather utility)\n\
         Writing to: {}\n\
         Gather interval: {} seconds\n",
        output_file, interval_secs
    );

    loop {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        println!("Gathering new interval at ts_epoch={}", now);

        for entry in fs::read_dir("/proc")? {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let fname = entry.file_name();
            let pid = match fname.to_str().and_then(|s| s.parse::<u32>().ok()) {
                Some(id) => id,
                None => continue,
            };

            // ---- SAFE PROC INFO READ (with timeout) ----
            let proc_info_opt = timeout_retry(
                move || gather_proc_info(pid),
                Duration::from_secs(2),
                2,
            );

            let proc_info = match proc_info_opt {
                Some(info) => info,
                None => {
                    eprintln!("WARN: Skipping PID {} (timeout)", pid);
                    continue;
                }
            };

            // ---- Write main process row ----
            wtr.serialize(CsvRow {
                ts_epoch: now,
                pid,
                ppid: proc_info.ppid,
                tid: pid,
                comm: proc_info.comm.clone(),
                state: proc_info.state.clone(),
                utime: proc_info.utime,
                stime: proc_info.stime,
                num_threads: Some(proc_info.num_threads),
                vmrss_kb: Some(proc_info.vmrss_kb),
                vm_size_kb: Some(proc_info.vmsize_kb),
                read_bytes: proc_info.read_bytes,
                write_bytes: proc_info.write_bytes,
                cmdline: proc_info.cmdline.clone(),
            })?;

            // ---- Threads ----
            let task_path = format!("/proc/{}/task", pid);
            if let Ok(task_dir) = fs::read_dir(&task_path) {
                for task_entry in task_dir {
                    let task_entry = match task_entry {
                        Ok(e) => e,
                        Err(_) => continue,
                    };
                    let tid = match task_entry
                        .file_name()
                        .to_str()
                        .and_then(|s| s.parse::<u32>().ok())
                    {
                        Some(id) if id != pid => id,
                        _ => continue,
                    };

                    let thread_info_opt = timeout_retry(
                        move || gather_thread_info(pid, tid),
                        Duration::from_secs(2),
                        2,
                    );

                    let thread_info = match thread_info_opt {
                        Some(info) => info,
                        None => {
                            eprintln!("WARN: Skipping TID {} (PID {}) timeout", tid, pid);
                            continue;
                        }
                    };

                    wtr.serialize(CsvRow {
                        ts_epoch: now,
                        pid,
                        ppid: proc_info.ppid,
                        tid,
                        comm: thread_info.comm,
                        state: thread_info.state,
                        utime: thread_info.utime,
                        stime: thread_info.stime,
                        num_threads: None,
                        vmrss_kb: None,
                        vm_size_kb: None,
                        read_bytes: proc_info.read_bytes,   // inherited from procfs
                        write_bytes: proc_info.write_bytes, // inherited
                        cmdline: String::new(),
                    })?;
                }
            }
        }

        wtr.flush()?; // ensure all buffered rows are written each interval
        println!("Sleeping {} seconds...", interval_secs);
        thread::sleep(Duration::from_secs(interval_secs));
    }
}

// Timeout helper: run a closure with a timeout/retry for reading procfs
fn timeout_retry<F, T>(func: F, timeout: Duration, retries: usize) -> Option<T>
where
    F: Send + 'static + Clone + Fn() -> Option<T>,
    T: Send + 'static,
{
    for attempt in 0..retries {
        let f = func.clone();
        if let Some(res) = read_with_timeout(f, timeout) {
            return Some(res);
        }

        if attempt == 0 {
            eprintln!("WARN: Timeout on attempt 1, retrying...");
        }
    }
    None
}

fn read_with_timeout<F, T>(func: F, timeout: Duration) -> Option<T>
where
    F: Send + 'static + FnOnce() -> Option<T>,
    T: Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel();
    spawn(move || {
        let _ = tx.send(func());
    });

    match rx.recv_timeout(timeout) {
        Ok(val) => val,
        Err(_) => None,
    }
}

// ------- Helpers for file naming and time -------

fn get_hostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknownhost".to_string())
}

fn get_time_string() -> String {
    let now = Local::now();
    format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}",
        now.year(),
        now.month(),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}

// -------- Data extractors for procs and threads --------

struct ProcInfo {
    comm: String,
    state: String,
    ppid: u32,
    utime: u64,
    stime: u64,
    num_threads: u32,
    vmrss_kb: u64,
    vmsize_kb: u64,
    read_bytes: u64,
    write_bytes: u64,
    cmdline: String,
}

struct ThreadInfo {
    comm: String,
    state: String,
    utime: u64,
    stime: u64,
}

fn parse_stat_line(stat_line: &str) -> Option<(String, Vec<String>)> {
    let start = stat_line.find('(')?;
    let end = stat_line.rfind(')')?;
    let comm = stat_line[start + 1..end].to_string();
    let after = &stat_line[end + 2..];
    let rest = after.split_whitespace().map(|s| s.to_string()).collect();
    Some((comm, rest))
}

fn gather_proc_info(pid: u32) -> Option<ProcInfo> {
    let stat = fs::read_to_string(format!("/proc/{}/stat", pid)).ok()?;
    let (comm, fields) = parse_stat_line(&stat)?;

    if fields.len() < 22 { return None; }

    let state = fields[0].clone();
    let ppid = fields[1].parse().ok()?;
    let utime = fields[11].parse().ok()?;
    let stime = fields[12].parse().ok()?;
    let num_threads = fields[17].parse().ok()?;

    let vsize_bytes: u64 = fields[20].parse().ok()?;
    let vmsize_kb = vsize_bytes / 1024;

    let rss_pages: u64 = fields[21].parse().ok()?;
    let vmrss_kb = rss_pages * page_size_kb()?;

    let mut read_bytes = 0;
    let mut write_bytes = 0;
    if let Ok(file) = File::open(format!("/proc/{}/io", pid)) {
        for line in BufReader::new(file).lines().flatten() {
            if let Some(val) = line.strip_prefix("read_bytes:") {
                read_bytes = val.trim().parse().unwrap_or(0);
            }
            if let Some(val) = line.strip_prefix("write_bytes:") {
                write_bytes = val.trim().parse().unwrap_or(0);
            }
        }
    }

    let cmdline = fs::read(format!("/proc/{}/cmdline", pid))
        .ok()
        .and_then(|data| {
            if data.is_empty() { None }
            else {
                Some(
                    data.split(|&b| b == 0)
                        .filter(|s| !s.is_empty())
                        .map(|s| String::from_utf8_lossy(s).to_string())
                        .collect::<Vec<_>>()
                        .join(" ")
                )
            }
        })
        .unwrap_or_default();

    Some(ProcInfo {
        comm, state, ppid, utime, stime, num_threads,
        vmrss_kb, vmsize_kb, read_bytes, write_bytes, cmdline
    })
}

fn gather_thread_info(pid: u32, tid: u32) -> Option<ThreadInfo> {
    let stat = fs::read_to_string(format!("/proc/{}/task/{}/stat", pid, tid)).ok()?;
    let (comm, fields) = parse_stat_line(&stat)?;
    if fields.len() < 14 { return None; }
    Some(ThreadInfo {
        comm,
        state: fields[0].clone(),
        utime: fields[11].parse().ok()?,
        stime: fields[12].parse().ok()?,
    })
}

// Get OS page size (in kB)
fn page_size_kb() -> Option<u64> {
    unsafe {
        let ps = libc::sysconf(libc::_SC_PAGESIZE);
        if ps > 0 { Some((ps as u64) / 1024) } else { None }
    }
}

