import java.io.File
import java.net.URI
import java.security.MessageDigest

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

val repoRoot: File = rootProject.projectDir.parentFile

/** The workspace's version (Cargo.toml `[workspace.package] version`): the app's versionName. */
val mdmVersion: String = repoRoot.resolve("Cargo.toml").readLines()
    .first { it.trim().startsWith("version = ") }
    .substringAfter('"').substringBefore('"')

/** 0.5.2 → 5002: major * 1_000_000 + minor * 1000 + patch, times 10 for per-ABI codes. */
val mdmVersionCode: Int = mdmVersion.split('.').map { it.takeWhile(Char::isDigit).toIntOrNull() ?: 0 }
    .let { (it.getOrElse(0) { 0 } * 1_000_000 + it.getOrElse(1) { 0 } * 1000 + it.getOrElse(2) { 0 }) * 10 }

/** The ABIs libmdm.so is built for: `-Pmdm.abis=arm64-v8a,armeabi-v7a,x86_64` (arm64 alone by default). */
val abis: List<String> = (findProperty("mdm.abis") as String? ?: "arm64-v8a").split(',').map(String::trim)

/** rustls-platform-verifier's Android component must be the version Cargo.lock has. */
val verifierVersion: String = repoRoot.resolve("Cargo.lock").readLines().let { lines ->
    val at = lines.indexOfFirst { it.trim() == "name = \"rustls-platform-verifier-android\"" }
    check(at >= 0) { "rustls-platform-verifier-android isn't in Cargo.lock" }
    lines.drop(at + 1).first { it.trim().startsWith("version = ") }.substringAfter('"').substringBefore('"')
}

android {
    namespace = "com.misterdownloadmanager.android"
    compileSdk = 36
    // The NDK cargo-ndk builds libmdm.so with; AGP strips the libraries with its tools.
    ndkVersion = "28.2.13676358"

    defaultConfig {
        applicationId = "com.misterdownloadmanager.android"
        minSdk = 26
        targetSdk = 36
        versionCode = mdmVersionCode
        versionName = mdmVersion
        ndk { abiFilters += abis }
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            // Release signing is the build workflow's (the maintainer's keys); here the debug key.
            signingConfig = signingConfigs.getByName("debug")
        }
        debug {
            applicationIdSuffix = ".debug"
        }
    }

    splits {
        abi {
            isEnable = abis.size > 1
            reset()
            include(*abis.toTypedArray())
            isUniversalApk = true
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures {
        // GameActivity's own glue is android-activity's (crates/android): no prefab C++ layer.
        prefab = false
    }

    sourceSets {
        getByName("main") {
            // yt-dlp's Python, ffmpeg and QuickJS (unpackHelpers, below).
            jniLibs.srcDir(layout.buildDirectory.dir("generated/helperJniLibs"))
            jniLibs.srcDir(layout.buildDirectory.dir("generated/sevenZipJniLibs"))
            // The licences the app shows (noticeAssets and build7zip, below).
            assets.srcDir(layout.buildDirectory.dir("generated/noticeAssets"))
            assets.srcDir(layout.buildDirectory.dir("generated/sevenZipAssets"))
        }
    }

    packaging {
        // The helper programs (python, ffmpeg) are executables packed as lib*.so: they must be
        // extracted to nativeLibraryDir to run (Android 10+ runs no file the app writes itself).
        jniLibs {
            useLegacyPackaging = true
            // Zips under a library's name (the helpers' files), not programs: nothing to strip.
            keepDebugSymbols += "**/*.zip.so"
        }
    }
}

kotlin {
    compilerOptions { jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17) }
}

dependencies {
    // The activity the app draws in (android-activity's `game-activity` backend): its version is
    // the one android-activity supports (its README).
    implementation("androidx.games:games-activity:4.4.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.core:core-ktx:1.17.0")
    implementation("org.rustls:rustls-platform-verifier:$verifierVersion")
}

// libmdm.so from crates/android, built with cargo-ndk into src/main/jniLibs before the native
// libraries are merged. `-Pmdm.skipCargo=true` uses what is there (CI builds it in its own step).
val cargoProfile: String = findProperty("mdm.cargoProfile") as String? ?: "release"
val cargoBuild by tasks.registering(Exec::class) {
    group = "build"
    description = "Builds libmdm.so (crates/android) for ${abis.joinToString()} with cargo-ndk"
    workingDir = repoRoot
    // libmdm.so needs libc++_shared.so (BoringSSL is C++): copied beside it.
    val args = mutableListOf("cargo", "ndk", "--platform", "26", "--link-libcxx-shared")
    abis.forEach { args += listOf("-t", it) }
    args += listOf("-o", projectDir.resolve("src/main/jniLibs").path, "build", "-p", "mdm-android")
    if (cargoProfile == "release") args += "--release" else args += listOf("--profile", cargoProfile)
    commandLine(args)
    onlyIf { (findProperty("mdm.skipCargo") as String?) != "true" }
}
tasks.matching { it.name.startsWith("merge") && it.name.endsWith("JniLibFolders") }.configureEach {
    dependsOn(cargoBuild)
}

// The helper programs video downloads need, as youtubedl-android (GPL-3.0) builds them for
// Android: Python 3.12 (libpython.so, its standard library in libpython.zip.so), QuickJS
// (libqjs.so), ffmpeg and ffprobe (libffmpeg.so, libffprobe.so, their libraries in
// libffmpeg.zip.so). Only their native files are taken, not the libraries' Java code: the core
// runs them itself (crates/tools/src/android.rs).
val helpers: Configuration by configurations.creating { isTransitive = false }
dependencies {
    helpers("io.github.junkfood02.youtubedl-android:library:0.18.1@aar")
    helpers("io.github.junkfood02.youtubedl-android:ffmpeg:0.18.1@aar")
}
val unpackHelpers by tasks.registering(Sync::class) {
    description = "Takes the helper programs' native files out of the youtubedl-android AARs"
    from(provider { helpers.files.map { zipTree(it) } }) {
        include("jni/**")
        exclude("**/libaria2c*")
        eachFile { path = path.removePrefix("jni/") }
    }
    includeEmptyDirs = false
    into(layout.buildDirectory.dir("generated/helperJniLibs"))
}
tasks.matching { it.name.startsWith("merge") && it.name.endsWith("JniLibFolders") }.configureEach {
    dependsOn(unpackHelpers, build7zip)
}

// 7-Zip for 7z and RAR archives (crates/extract runs it as its own program, as on the desktop:
// UnRAR's licence keeps it out of a GPL program's code). Built here from 7-Zip's own source with
// the NDK, against Android's libc: its official Linux builds are linked with glibc, whose system
// calls Android's app sandbox (seccomp) kills with SIGSYS. The source is checked against its
// pinned SHA-256 (the same Termux pins for its 7zip package).
val sevenZipVersion = "26.06"
val sevenZipSource = "7z${sevenZipVersion.replace(".", "")}-src.tar.xz" to
    "a4e374c8f864f51df48c2cfd0e1413ec3b2ea030deae852d6a655402ebb61ac4"
val sevenZipTarget = mapOf(
    "arm64-v8a" to "aarch64-linux-android26",
    "armeabi-v7a" to "armv7a-linux-androideabi26",
    "x86_64" to "x86_64-linux-android26",
)
val ndkDir = androidComponents.sdkComponents.ndkDirectory
// `-Pmdm.sevenZipRar=false` builds 7-Zip without its RAR code (7-Zip's DISABLE_RAR): no unRAR
// code in the APK, and no RAR archives extracted.
val sevenZipRar = (findProperty("mdm.sevenZipRar") as String?) != "false"
val build7zip by tasks.registering {
    description = "Builds 7-Zip's 7zz from source with the NDK as lib7zz.so for each ABI"
    val out = layout.buildDirectory.dir("generated/sevenZipJniLibs")
    val licence = layout.buildDirectory.file("generated/sevenZipAssets/licenses/7-Zip.txt")
    val work = layout.buildDirectory.dir("7zip")
    outputs.dir(out)
    outputs.file(licence)
    inputs.property("abis", abis)
    inputs.property("source", sevenZipSource)
    inputs.property("rar", sevenZipRar)
    doLast {
        val (name, sha) = sevenZipSource
        val archive = work.get().file(name).asFile
        archive.parentFile.mkdirs()
        fun digest(f: File) = MessageDigest.getInstance("SHA-256").digest(f.readBytes())
            .joinToString("") { "%02x".format(it) }
        if (!archive.exists() || digest(archive) != sha) {
            URI("https://github.com/ip7z/7zip/releases/download/$sevenZipVersion/$name").toURL()
                .openStream().use { i -> archive.outputStream().use { o -> i.copyTo(o) } }
        }
        check(digest(archive) == sha) { "$name doesn't match its pinned SHA-256" }
        val src = work.get().dir("src").asFile
        src.deleteRecursively()
        src.mkdirs()
        providers.exec { commandLine("tar", "-xJf", archive.path, "-C", src.path) }.result.get()
        val host = if (System.getProperty("os.name").startsWith("Mac")) "darwin-x86_64" else "linux-x86_64"
        val bin = ndkDir.get().dir("toolchains/llvm/prebuilt/$host/bin").asFile
        val bundle = src.resolve("CPP/7zip/Bundles/Alone2")
        abis.forEach { abi ->
            val target = sevenZipTarget[abi] ?: return@forEach
            // arm64 takes 7-Zip's own assembly (GNU syntax, as clang reads it); the others its C.
            val mak = if (abi == "arm64-v8a") "../../cmpl_clang_arm64.mak" else "../../cmpl_clang.mak"
            // -D_GNU_SOURCE for bionic's sched.h; libc++ linked in, so 7zz needs no other library;
            // warnings not errors, since a newer NDK may warn where 7-Zip's compilers don't.
            val flags = "--target=$target -D_GNU_SOURCE -Wno-unused-command-line-argument"
            val make = providers.exec {
                workingDir = bundle
                isIgnoreExitValue = true
                commandLine(
                    "make", "-j${Runtime.getRuntime().availableProcessors()}", "-f", mak,
                    "CC=${bin.resolve("clang")} $flags",
                    "CXX=${bin.resolve("clang++")} $flags -static-libstdc++",
                    "CFLAGS_WARN_WALL=-Wall", "O=b/$abi", "LIB2=-ldl",
                    *(if (sevenZipRar) emptyArray() else arrayOf("DISABLE_RAR=1")),
                )
            }
            check(make.result.get().exitValue == 0) {
                "7-Zip didn't build for $abi:\n" + make.standardError.asText.get().takeLast(8000)
            }
            val dir = out.get().dir(abi).asFile.apply { mkdirs() }
            bundle.resolve("b/$abi/7zz").copyTo(File(dir, "lib7zz.so"), overwrite = true)
        }
        // Its licence, carried in the APK.
        src.resolve("DOC/License.txt").copyTo(licence.get().asFile, overwrite = true)
    }
}

// The licences the APK carries: MDM's own; 7-Zip's comes from its source (build7zip).
val noticeAssets by tasks.registering(Copy::class) {
    from(repoRoot.resolve("LICENSE")) { rename { "GPL-3.0.txt" } }
    into(layout.buildDirectory.dir("generated/noticeAssets/licenses"))
}
tasks.matching { it.name.startsWith("merge") && it.name.endsWith("Assets") }.configureEach {
    dependsOn(noticeAssets, build7zip)
}
// Lint reads every source folder too, generated ones included (a release build's
// generateReleaseLintVitalReportModel and lintVitalAnalyzeRelease, ./gradlew lint's tasks).
tasks.matching { it.name.contains("Lint") || it.name.startsWith("lint") }.configureEach {
    dependsOn(noticeAssets, build7zip, unpackHelpers)
}
