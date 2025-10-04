plugins {
  id("com.android.application")
  id("org.jetbrains.kotlin.android")
}

android {
  namespace = "com.example.hellocompose"
  compileSdk = 34

  defaultConfig {
    applicationId = "com.example.hellocompose"
    minSdk = 24
    targetSdk = 34
    versionCode = 1
    versionName = "1.0"
  }
  val devApiUrl = providers
    .environmentVariable("DEV_API_URL")
    .orElse("http://192.168.1.81:8080")
    .get()

  buildTypes {
    debug {
      val devApiUrl = providers.environmentVariable("DEV_API_URL").orElse("http://192.168.1.81:8080").get()
      buildConfigField("String", "BASE_URL", "\"$devApiUrl\"")
    }
    release {
      buildConfigField("String", "BASE_URL", "\"https://blaz.matmoa.eu\"")
      isMinifyEnabled = false
      proguardFiles(
        getDefaultProguardFile("proguard-android-optimize.txt"),
        "proguard-rules.pro"
      )
    }
  }

  compileOptions {
    sourceCompatibility = JavaVersion.VERSION_17
    targetCompatibility = JavaVersion.VERSION_17
  }
  kotlin {
    jvmToolchain(17)
  }

  packaging {
    resources.excludes += "/META-INF/{AL2.0,LGPL2.1}"
  }
}

// Compose configuration
android {
  buildFeatures { 
    compose = true
    buildConfig = true
  }
  composeOptions { kotlinCompilerExtensionVersion = "1.5.14" }
}

dependencies {
  // Retrofit + Gson + OkHttp (for your Api.kt)
  implementation("com.squareup.retrofit2:retrofit:2.11.0")
  implementation("com.squareup.retrofit2:converter-gson:2.11.0")
  implementation("com.squareup.okhttp3:logging-interceptor:4.12.0")

  val composeBom = platform("androidx.compose:compose-bom:2024.06.00")
  implementation(composeBom)
  androidTestImplementation(composeBom)

  implementation("androidx.activity:activity-compose:1.9.2")
  implementation("androidx.compose.ui:ui")
  implementation("androidx.compose.ui:ui-tooling-preview")
  implementation("androidx.compose.material3:material3")
  implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.8.6")
  implementation("androidx.core:core-ktx:1.13.1")

  debugImplementation("androidx.compose.ui:ui-tooling")
}
