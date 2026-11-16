plugins {
    id("com.android.application") version "8.13.2" apply false
    id("org.jetbrains.kotlin.android") version "2.3.21" apply false
    id("org.jetbrains.kotlin.plugin.compose") version "2.3.21" apply false
    id("org.jetbrains.kotlin.plugin.serialization") version "2.3.21" apply false
    // Screenshots of the screens on the JVM, without a device (src/test/.../Shots.kt).
    id("io.github.takahirom.roborazzi") version "1.79.0" apply false
}
