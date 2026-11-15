# Release Notes

## [Unreleased]

### Added

- **Split videos based on chapters**:
  - Automatically split downloaded videos into separate files based on YouTube's or other video services' chapters.
  - Stream copy only (no re-encoding): splits the video losslessly and quickly without re-encoding the video or audio streams.
  - Chapter file naming convention: `<filename> - <chapter name>.<ext>` (e.g. `My Video - Intro Chapter.mp4`, respecting any custom filename specified in the download dialog and grouping split chapters neatly together).
  - Added "Split into chapters" option in the Download Video window beside "Embed chapters".
  - Full CLI support via the `--split-chapters` option in `video-get`.
  - Download file cleanup: Chapter files associated with the video are detected and removed when deleting a download along with its files.
