#!/usr/bin/env python3

# Laurence Oberman <loberman@redhat.com>

import sys

# Example use
# serverstats_grab -pD cwypla-584-20251205-044446.dat | ~/tools/short_disk_report.py | grep -e Device -e sdb
# Fields you want, in this order
DESIRED_FIELDS = [
    "Device", "Time", "Δt", "ΔReads", "ΔWrites",
    "Qlen", "r/s", "w/s", "rd_kB/s", "wr_kB/s",
    "await_rd(ms)", "await_wr(ms)"
]

header_found = False
field_indices = []

for line in sys.stdin:
    if not header_found:
        # Find the header line
        headers = [h.strip() for h in line.strip().split()]
        if all(field in headers for field in DESIRED_FIELDS):
            header_found = True
            field_indices = [headers.index(f) for f in DESIRED_FIELDS]
            # Print your custom header
            print(' '.join(f"{field:>12}" for field in DESIRED_FIELDS))
        continue
    if not header_found or not line.strip() or line.startswith("---"):
        continue
    parts = line.strip().split()
    if len(parts) < max(field_indices) + 1:
        continue
    print(' '.join(f"{parts[idx]:>12}" for idx in field_indices))

