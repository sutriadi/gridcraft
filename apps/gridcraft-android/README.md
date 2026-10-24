# GridCraft for Android (tablets and foldables)

Status: **first skeleton, never run on a device.** The Rust side is checked and its bridge tests pass on a
Linux host; the Android build (NDK, Gradle, Kotlin) was written without being able to compile it.

## How it works

- `src/lib.rs` runs the same `WordApp` as desktop/web through eframe's `NativeActivity` backend.
- Android's open/save pickers (Storage Access Framework) are Kotlin-only, so `src/bridge.rs` and
  `android/.../MainActivity.kt` exchange small files in `<filesDir>/bridge` (protocol in `bridge.rs`).
  This keeps the Rust side free of `unsafe`/JNI except the single `android_main` entry point.
- The manifest handles `configChanges` + `resizeableActivity`, so folding/unfolding and split-screen
  resize the window without restarting (and losing the open workbook).
- `crates/fonts`: Android's system font folders (`/system/fonts`, …) are scanned.

## Build

On Windows, first check the machine (read-only, changes nothing):

```powershell
powershell -ExecutionPolicy Bypass -File apps\gridcraft-android\check-env.ps1
```

Then:

```sh
rustup target add aarch64-linux-android x86_64-linux-android
cargo install cargo-ndk
export ANDROID_HOME=… ANDROID_NDK_HOME=…
apps/gridcraft-android/build-android.sh --apk     # needs `gradle` (or add a wrapper)
cargo test -p gridcraft-android                   # bridge tests, host only
```

## Not done yet

- Touch UI: the ribbon is the desktop one. Needs bigger hit targets, a context toolbar for selections,
  long-press menu, two-finger zoom, and checking the on-screen keyboard/IME with `TextEdit`.
- Foldable posture (tabletop/book mode via Jetpack WindowManager), stylus, external keyboard shortcuts.
- Open-with / share-to-GridCraft intents (`ACTION_VIEW`), recent files, clipboard hooks, printing (PDF share).
- Real-device checks: wgpu on Vulkan vs GLES, performance, APK size, signing for release.
