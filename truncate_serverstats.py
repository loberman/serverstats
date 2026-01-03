#!/usr/bin/env python3
# Laurence Oberman <loberman@redhat.com> 
#Usage
# truncate_serverstats.py cwypla-584-20251205-044446.dat truncated.cwypla-584-20251205-044446.dat --from 10:00:00 --to 12:00:00

import argparse
import sys
import os
from datetime import datetime, time

def parse_time_hms(timestr):
    # Require strict HH:MM:SS format
    try:
        t = datetime.strptime(timestr, "%H:%M:%S").time()
        return t
    except ValueError:
        print(f"ERROR: Time '{timestr}' must be in HH:MM:SS format, e.g. 12:33:01", file=sys.stderr)
        sys.exit(1)

def epoch_to_hms(epoch):
    # Returns time as a datetime.time object
    return datetime.fromtimestamp(int(epoch)).time()

def in_time_window(row_time, from_time, to_time):
    # If either bound is None, treat as unbounded
    if from_time and row_time < from_time:
        return False
    if to_time and row_time > to_time:
        return False
    return True

def main():
    parser = argparse.ArgumentParser(
        description="Truncate serverstats_grab .dat file to a wallclock time window."
    )
    parser.add_argument("input_file", help=".dat file to process")
    parser.add_argument("output_file", help="output file for truncated data")
    parser.add_argument("--from", dest="from_time", help="start time (HH:MM:SS)")
    parser.add_argument("--to", dest="to_time", help="end time (HH:MM:SS)")

    args = parser.parse_args()

    from_time = parse_time_hms(args.from_time) if args.from_time else None
    to_time   = parse_time_hms(args.to_time) if args.to_time else None

    # Open input and output files
    with open(args.input_file, "r") as f_in, open(args.output_file, "w") as f_out:
        for line in f_in:
            if line.startswith("#TYPE") or line.startswith("#"):
                f_out.write(line)
                continue
            cols = line.strip().split(",")
            if len(cols) < 2:
                continue
            # cols[1] is ts_epoch (as written by serverstats_grab)
            try:
                row_time = epoch_to_hms(cols[1])
            except Exception:
                continue
            if in_time_window(row_time, from_time, to_time):
                f_out.write(line)

    print(f"Done. Wrote: {args.output_file}")

if __name__ == "__main__":
    main()

