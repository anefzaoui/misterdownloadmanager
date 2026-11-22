# Multi-mirror downloads

## Storage

Each engine resume state keeps the original `url` for compatibility and a
`primary_url` for the mirror model. Old `.mdm.json` files load without
`primary_url` or `mirrors`; loading a legacy state copies `url` into
`primary_url` and starts with an empty mirror list. The app list keeps the
same mirror configuration alongside the existing download row and writes it
back into the resume state beside the part file.

## Credentials

Each mirror has optional Referer, Cookie, and User-Agent values. The existing
download header handling supplies these fields without storing them in the
resume JSON or event log. Cookies and Authorization values are never written
to mirror status text or logs.

## Same-file check

Before enabling a mirror, MDM sends a range probe (`Range: bytes=0-0`) and/or
HEAD request. It requires the reported whole-file size to match the task and
a `206` range answer. ETag and Last-Modified are comparison hints only and are
never required.

## Segmentation and failover

The existing planner owns every byte-range segment. Finished segments remain
untouched. A saved segment records only its source mirror index and retry
count; unfinished segments are assigned whole to a healthy mirror. A failing
mirror's unfinished segment is released back to the planner and a bounded
retry budget moves it to another mirror. Mirror failures never split or
re-download a completed segment.

## URL changes

Changing the primary URL first probes the new address against the task size
and range support. Rejection leaves the old primary URL and all existing
resume segments unchanged. Acceptance updates the primary source; existing
finished ranges stay on disk and unfinished ranges may continue or rebalance
without restarting the file.
