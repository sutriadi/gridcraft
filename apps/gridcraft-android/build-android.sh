#!/usr/bin/env bash
# Builds libgridcraft_android.so for the Android ABIs and, with --apk, a debug APK.
# Needs: rustup targets, cargo-ndk, the Android SDK + NDK (ANDROID_NDK_HOME), and Gradle.
#   rustup target add aarch64-linux-android x86_64-linux-android
#   cargo install cargo-ndk
set -euo pipefail
cd "$(dirname "$0")"
repo_root="$(cd ../.. && pwd)"
jni="android/app/src/main/jniLibs"

(cd "$repo_root" && cargo ndk -t arm64-v8a -t x86_64 -o "apps/gridcraft-android/$jni" build --release -p gridcraft-android)

if [[ "${1:-}" == "--apk" ]]; then
    (cd android && gradle :app:assembleDebug)
    echo "APK: $(pwd)/android/app/build/outputs/apk/debug/app-debug.apk"
fi
