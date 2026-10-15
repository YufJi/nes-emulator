// Versions are pinned rather than resolved dynamically: the front end is built
// by hand and by CI, and both should run the same compiler and the same
// Compose runtime. AGP 8.6 needs Gradle 8.7 (see `gradle/wrapper`).
plugins {
    id("com.android.application") version "8.6.0" apply false
    id("org.jetbrains.kotlin.android") version "1.9.0" apply false
}
