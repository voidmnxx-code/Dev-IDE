plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

// ---- Rust core: cargo-ndk + UniFFI Kotlin bindings ---------------------
val rustDir = rootProject.projectDir
val jniOut = layout.projectDirectory.dir("src/main/jniLibs")
val bindingsOut = layout.buildDirectory.dir("generated/uniffi/kotlin")

val buildRustCore by tasks.registering(Exec::class) {
    group = "rust"
    description = "Cross-compiles ide_core for arm64-v8a (16 KB page aligned)."
    workingDir = rustDir
    environment("RUSTFLAGS", "-C link-arg=-Wl,-z,max-page-size=16384")
    commandLine(
        "cargo", "ndk", "-t", "arm64-v8a", "-o", jniOut.asFile.absolutePath,
        "build", "--release", "-p", "ide_core",
    )
}

// UniFFI reads its metadata from the library's symbols, and the stripped Android
// release .so has none. So bindings are generated from an unstripped host build
// of the same crate (the interface is identical on every target).
val hostLibExt = if (System.getProperty("os.name").startsWith("Mac")) "dylib" else "so"

val buildHostCore by tasks.registering(Exec::class) {
    group = "rust"
    workingDir = rustDir
    commandLine("cargo", "build", "-p", "ide_core")
}

val generateBindings by tasks.registering(Exec::class) {
    group = "rust"
    description = "Generates UniFFI Kotlin bindings."
    dependsOn(buildHostCore)
    workingDir = rustDir
    // Declared as an output so Gradle orders it before Kotlin compilation.
    outputs.dir(bindingsOut)
    doFirst { bindingsOut.get().asFile.deleteRecursively() }
    commandLine(
        "cargo", "run", "--bin", "uniffi-bindgen", "--",
        "generate", "--library", "target/debug/libide_core.$hostLibExt",
        "--language", "kotlin", "--no-format", "--out-dir", bindingsOut.get().asFile.absolutePath,
    )
    doLast {
        val kt = bindingsOut.get().asFile.walkTopDown().filter { it.extension == "kt" }.toList()
        check(kt.isNotEmpty()) { "uniffi-bindgen produced no Kotlin files" }
    }
}

android {
    namespace = "dev.ide.app"
    compileSdk = 35

    defaultConfig {
        applicationId = "dev.ide.app"
        minSdk = 26
        targetSdk = 35 // Play requires API 35+; no targetSdk-28 tricks.
        versionCode = 1
        versionName = "0.1.0"
        ndk { abiFilters += "arm64-v8a" }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
    buildFeatures { compose = true }
    packaging {
        jniLibs { useLegacyPackaging = true } // extractNativeLibs=true: tools can be exec'd later
    }
    sourceSets.getByName("main").java.srcDir(generateBindings.map { bindingsOut.get() })
}

tasks.named("preBuild") { dependsOn(buildRustCore, generateBindings) }

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2024.12.01")
    implementation(composeBom)
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.foundation:foundation")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.7")
    implementation("androidx.core:core-ktx:1.15.0")
    implementation("net.java.dev.jna:jna:5.14.0@aar") // required by UniFFI bindings
}
