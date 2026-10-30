# Mister Download Manager (MDM) — agent guide

Mister Download Manager (MDM) is a download manager for Windows in Rust (egui + storytold
craft-libs), laid out the way Internet Download Manager users expect. Start by reading
[README.md](README.md) and [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Rules

1. **Clean room.** MDM contains no code, binaries, artwork, icons, sounds or text from
   Internet Download Manager or any other download manager. Never disassemble, decompile,
   debug-attach or patch any IDM file, and never bypass its licensing or trial. Behaviour is
   learned only from public documentation, screenshots and black-box use of a licensed copy.
2. Every user-visible text goes through `crate::i18n`; run `python tools/i18n/extract.py`
   after adding one.
3. Windows-only calls live in `crates/app/src/win32.rs`, their Linux and macOS counterparts in
   `unix.rs`; the rest of the workspace is safe, portable Rust (`unsafe_code = "deny"`). A
   feature works on every system, or says why it can't (`cfg` with a comment).
4. Fix root causes, keep data contracts aligned between producers and consumers, and verify the
   whole system end to end after each change.

## Commands

- `cargo build` / `cargo test` / `cargo clippy --all-targets` / `cargo fmt --all` — workspace.
- `tools/dev/check-cross.ps1` — clippy for Linux and macOS from Windows (needs Zig, see the
  script); `node tools/extension/e2e.mjs` — the extension in real headless browsers.
- `cargo run -p mdm-labserver -- --faults "rate=2M"` — test server (GET / lists the faults).
- `cargo run -p mdm-cli -- get <url> -o <dir>` — headless download with progress.
- `tools/extension/pack.ps1` — build the extension for Chromium and Firefox into
  `target/extension/`; `node tools/extension/test.mjs` — its logic against fake browser APIs.
- `tools/installer/build.ps1` — the Windows installer and portable zip into `target/dist/`.
- `tools/installer/build-linux.sh` / `build-macos.sh` — the experimental Linux (AppImage, tar.gz,
  .deb, .rpm) and macOS (.dmg) downloads; run by `release.yml` on GitHub's runners.
- Windows: if `cargo` isn't on PATH in an old shell, prefix
  `$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"`.

## Maintainer notes

The maintainer's private notes, when present, add to these rules:

@notes/CLAUDE.private.md
