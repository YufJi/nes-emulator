// One module: the TV app. The Rust core arrives as a prebuilt `jniLibs/` entry
// and a generated Kotlin binding, both produced by `build-core.sh`, so there is
// nothing for Gradle to compile from Rust and no NDK step in here.
pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "nes-tv"
include(":app")
