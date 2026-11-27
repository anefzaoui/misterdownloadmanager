# Third-party files

No prebuilt programs or libraries are kept here: what the build needs comes from its official
source and is checked against the checksum its source publishes.

- **Gradle's wrapper** (`gradle/wrapper/gradle-wrapper.jar`): fetched by `gradlew` and
  `gradlew.bat` from Gradle's repository at the version's tag, checked against
  `gradle-<version>-wrapper.jar.sha256` from services.gradle.org; Gradle itself is checked against
  `distributionSha256Sum` (`gradle-wrapper.properties`, Gradle's published checksum).
- **rustls-platform-verifier's Android component** (MIT OR Apache-2.0): from the Maven repository
  its project publishes on GitHub (`maven-archive` branch, `android-release-support/maven/`, as
  its README's Android setup says), checked against `gradle/verification-metadata.xml` (the
  SHA-1 the project publishes beside the file, and the file's SHA-256). Its version must be the one
  `Cargo.lock` gives `rustls-platform-verifier-android` (the Gradle build checks).
- **The helper programs** (Python, ffmpeg, QuickJS): from youtubedl-android's AARs on Maven
  Central, at build time, checked against `gradle/verification-metadata.xml`; **7-Zip**: built
  from its source, pinned by SHA-256 (`app/build.gradle.kts`).
