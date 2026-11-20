#!/bin/sh
# Build the debug APK: the Rust library for arm64 (cargo-ndk), then Gradle.
#   android/build.sh            -> android/app/build/outputs/apk/debug/app-debug.apk
#   android/build.sh release    -> android/app/build/outputs/apk/release/app-release.apk
# Only `-p aoto-android` builds for Android: the root package is the desktop app.
# The API level (-P 26) is the app's minSdk (app/build.gradle.kts) and the one bindgen
# is told in .cargo/config.toml: change all three together.
# Needs: ANDROID_HOME (default ~/Library/Android/sdk) with an NDK, JAVA_HOME (default:
# Android Studio's JDK), `rustup target add aarch64-linux-android`, `cargo install cargo-ndk`.
set -e
here=$(cd "$(dirname "$0")" && pwd)
export ANDROID_HOME="${ANDROID_HOME:-$HOME/Library/Android/sdk}"
export JAVA_HOME="${JAVA_HOME:-/Applications/Android Studio.app/Contents/jbr/Contents/Home}"
# bindgen (rquickjs's Android bindings) needs libclang; the Command Line Tools have one.
export LIBCLANG_PATH="${LIBCLANG_PATH:-/Library/Developer/CommandLineTools/usr/lib}"
cd "$here/.."
cargo ndk -t arm64-v8a -P 26 -o "$here/app/src/main/jniLibs" build --locked -p aoto-android --release
cd "$here"
if [ "$1" = release ]; then
    ./gradlew assembleRelease
    echo "APK: $here/app/build/outputs/apk/release/app-release.apk"
else
    ./gradlew assembleDebug
    echo "APK: $here/app/build/outputs/apk/debug/app-debug.apk"
fi
