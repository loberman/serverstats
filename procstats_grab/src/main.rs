// procstats_grab - Process Stats Gatherer & Analyzer (Main Entry)
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
    # main.rs

    Main entry for procstats_grab:
    - Handles CLI usage and help/usage display
    - Supports modes:
      - `-a <file>`: Analysis/tables/charts
      - `-p <file>`: Playback (print sample deltas)
      - `-g <interval>`: Gather mode
      - `-h`: Help/usage
*/

//! # procstats_grab Main Entry Point
//!
//! Command-line frontend for procstats_grab.
//! Handles gather, analyze, and playback modes.

// Increment as tol evolves
const VERSION_NUMBER: &str = "2.1.1";

mod analyze;
mod gather;

use std::env;

/// Prints usage/help for procstats_grab.
fn print_usage(prog: &str) {
    println!("procstats_gather {}", VERSION_NUMBER);
    println!("Usage:");
    println!("  {} -a <procstats_gather.csv>        # Analyze mode: tables & charts", prog);
    println!("  {} -p <procstats_gather.csv>        # Playback mode: print sample deltas", prog);
    println!("  {} -p <procstats_gather.csv> -wide  # Playback (wide): show full args at end", prog);
    println!("  {} -g <interval_secs>               # Gather mode (default: 60s)", prog);
    println!("  {} -h                               # Show this help/usage", prog);
    println!();
    println!("After running the -a analyze option you can cd to the directory 
    Then run this python lightweight web server and browse the analysis data: 
    python3 -m http.server 8080 

    Please note! playback | more or less will see a thread main stack panic on quit 
    This can be safely ignored, it is how stdout works with Rust.");

}
/// Entry point for procstats_grab.
///
/// Parses command-line arguments and dispatches to the selected mode:
/// - Gather mode (`-g`): Capture stats to CSV.
/// - Analyze mode (`-a`): Generate reports.
/// - Playback mode (`-pD`): Print deltas per sample.
fn main() -> std::io::Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 || args[1] == "-h" || args[1] == "--help" {
        print_usage(&args[0]);
        return Ok(());
    }

    match args[1].as_str() {
         "-a" => {
            if args.len() < 3 {
                eprintln!("Usage: {} -a <procstats_gather.csv>", args[0]);
                std::process::exit(1);
            }
            analyze::run_analysis(&args[2]).expect("Failed to analyze CSV");
         }
         "-p" => {
            if args.len() < 3 {
                eprintln!("Usage: {} -p <procstats_gather.csv> [-wide]", args[0]);
                std::process::exit(1);
            }
            let wide = args.len() > 3 && (args[3] == "-wide" || args[3] == "--wide" || args[3] == "-pwide");
            analyze::run_playback(&args[2], wide).expect("Failed to play back CSV");
         }
         "-g" => {
            // Parse gather interval if given (-g <seconds>)
            let interval_secs = if args.len() >= 3 {
                args[2].parse().unwrap_or(60u64)
            } else {
                60u64
            };
            gather::run_gather(interval_secs)?;
         }
        _ => {
            print_usage(&args[0]);
            std::process::exit(1);
        }
    }
    Ok(())
}

