# Third-party files

- `maven/org/rustls/rustls-platform-verifier/0.2.0`: the Android (Kotlin) component of the
  [rustls-platform-verifier](https://github.com/rustls/rustls-platform-verifier) crate (MIT OR
  Apache-2.0), copied unchanged from its `maven-archive` branch
  (`android-release-support/maven/`), SHA-1 `28b6b316c2749a7c94b4d19061f12c64759a0e27`. Its version
  must be the one `Cargo.lock` gives `rustls-platform-verifier-android` (the Gradle build checks).
  Kept here so builds need no access to GitHub's raw files.
- `app/src/main/res/font/archivo.ttf`: Archivo, the variable font (width and weight axes) the
  website uses (DESIGN.md), from [google/fonts](https://github.com/google/fonts/tree/main/ofl/archivo)
  `ofl/archivo/Archivo[wdth,wght].ttf`, unchanged. SIL Open Font License 1.1: `Archivo-OFL.txt`.
