plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

// The game list is a single file shared with the web front end, so it is copied
// into the assets at build time instead of being committed a second time:
// `web/public/games.json` stays the only source and the two can't drift.
val gameList = rootProject.file("../web/public/games.json")
val copyGameList = tasks.register<Copy>("copyGameList") {
    // `web/public/games.json` is committed; a checkout without it means the
    // script that generates it (`npm run build:games`) has never been run.
    onlyIf { gameList.exists() }
    from(gameList)
    into(layout.buildDirectory.dir("generated/games"))
}

android {
    // Matches the iOS app's bundle identifier, so the two front ends of the same
    // project are recognisably siblings.
    namespace = "com.example.nesapp"
    compileSdk = 34

    defaultConfig {
        applicationId = "com.example.nesapp"
        // Android TV started at API 21 and plenty of boxes still run it; the
        // Compose version below also floors at 21.
        minSdk = 21
        targetSdk = 34
        versionCode = 1
        versionName = "0.1.0"

        // The ABIs `build-core.sh` builds: 64-bit devices, 32-bit ARM for the
        // sets that run a 32-bit Android userspace on 64-bit silicon (Sony's,
        // among others — without it the APK does not install), 64-bit emulator
        // images, and 32-bit x86 for Google's own TV system images up to API 31.
        ndk {
            abiFilters += listOf("arm64-v8a", "armeabi-v7a", "x86_64", "x86")
        }
    }

    buildTypes {
        release {
            // Kept readable: this is a study project, and nothing here ships to
            // a store with a size budget.
            isMinifyEnabled = false
        }
    }

    buildFeatures {
        compose = true
    }
    composeOptions {
        // Must stay in step with the Kotlin version above: 1.5.1 is the
        // compiler for Kotlin 1.9.0.
        kotlinCompilerExtensionVersion = "1.5.1"
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }

    sourceSets {
        getByName("main") {
            assets.srcDir(layout.buildDirectory.dir("generated/games"))
        }
    }

    packaging {
        resources.excludes += setOf(
            // The UniFFI/Kotlin metadata files duplicate what the classes carry.
            "META-INF/*.kotlin_module",
            "META-INF/AL2.0",
            "META-INF/LGPL2.1",
        )
    }
}

tasks.named("preBuild") {
    dependsOn(copyGameList)
}

dependencies {
    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.activity:activity-compose:1.9.2")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.8.5")

    implementation("androidx.compose.ui:ui:1.6.6")
    implementation("androidx.compose.ui:ui-graphics:1.6.6")
    implementation("androidx.compose.foundation:foundation:1.6.6")
    implementation("androidx.compose.material3:material3:1.2.1")

    // What the generated UniFFI bindings call the Rust core through. The AAR
    // variant is the one that carries the Android `.so`s, and 5.14 is the floor
    // UniFFI documents for passing structs by value.
    implementation("net.java.dev.jna:jna:5.14.0@aar")

    // For `games.json` and nothing else. It is here rather than `org.json`
    // because the parse has a unit test, and the framework's copy of `org.json`
    // is a stub outside a device.
    implementation("com.google.code.gson:gson:2.10.1")

    testImplementation("junit:junit:4.13.2")
}
