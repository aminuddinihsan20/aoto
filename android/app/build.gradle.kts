plugins {
    id("com.android.application")
}

android {
    namespace = "io.github.aminuddinihsan20.aoto"
    compileSdk = 36

    defaultConfig {
        applicationId = "io.github.aminuddinihsan20.aoto"
        // AAudio, which cpal plays through, needs API 26.
        minSdk = 26
        targetSdk = 36
        versionCode = 5
        versionName = "1.0.4"
        // The Rust library is built for arm64 only (phones, and the emulator on Apple silicon).
        ndk {
            abiFilters += "arm64-v8a"
        }
    }

    // A release is signed with the project's key when CI (or you) provide it: the key's file
    // and passwords in ANDROID_KEYSTORE_FILE, ANDROID_KEYSTORE_PASSWORD, ANDROID_KEY_ALIAS and
    // ANDROID_KEY_PASSWORD. Without them it is signed with the debug key, which installs fine
    // but can't update an app signed with another key.
    val keystore = System.getenv("ANDROID_KEYSTORE_FILE")?.takeIf { it.isNotEmpty() && file(it).isFile }
    signingConfigs {
        if (keystore != null) {
            create("release") {
                storeFile = file(keystore)
                storePassword = System.getenv("ANDROID_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("ANDROID_KEY_ALIAS")
                keyPassword = System.getenv("ANDROID_KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            signingConfig = if (keystore != null) signingConfigs.getByName("release") else signingConfigs.getByName("debug")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

dependencies {
    // The version android-activity 0.6 (in eframe/winit) is written against; don't add
    // its native "prefab" glue, android-activity has its own.
    implementation("androidx.games:games-activity:4.4.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    // Lets the sign-in WebView leave out the header that names the app.
    implementation("androidx.webkit:webkit:1.14.0")
}
