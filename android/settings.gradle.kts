pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
        // rustls-platform-verifier's Android component, from the Maven repository its project
        // publishes on GitHub (its README's Android setup), and only from there; its checksums are
        // in gradle/verification-metadata.xml.
        exclusiveContent {
            forRepository {
                maven {
                    url = uri("https://raw.githubusercontent.com/rustls/rustls-platform-verifier/maven-archive/android-release-support/maven/")
                }
            }
            filter { includeGroup("org.rustls") }
        }
    }
}

rootProject.name = "MisterDownloadManager"
include(":app")
