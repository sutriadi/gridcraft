plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "com.sutriadi.gridcraft"
    compileSdk = 36

    defaultConfig {
        applicationId = "com.sutriadi.gridcraft"
        minSdk = 26
        targetSdk = 36
        // Play needs a higher versionCode for every upload; CI passes its run number.
        versionCode = (System.getenv("GITHUB_RUN_NUMBER") ?: "1").toInt()
        versionName = "0.1.001"
        // Only the ABIs `build-android.sh` produces.
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
    }

    // Upload key: only configured when CI provides the keystore (see .github/workflows/android.yml).
    val keystorePath = System.getenv("ANDROID_KEYSTORE_FILE")
    signingConfigs {
        if (keystorePath != null) {
            create("release") {
                storeFile = file(keystorePath)
                storePassword = System.getenv("ANDROID_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("ANDROID_KEY_ALIAS")
                keyPassword = System.getenv("ANDROID_KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            signingConfig = signingConfigs.findByName("release")
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }

    // libgridcraft_android.so is copied here by build-android.sh (cargo-ndk).
    sourceSets["main"].jniLibs.srcDir("src/main/jniLibs")
}
