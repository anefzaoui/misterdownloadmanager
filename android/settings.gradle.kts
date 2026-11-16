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
        // rustls-platform-verifier's Android component (third_party/README.md).
        maven { url = uri("third_party/maven") }
    }
}

rootProject.name = "MisterDownloadManager"
include(":app")
