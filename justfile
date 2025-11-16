# Use bash with strict-ish options
set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

# ---- Config ----

binary_name := "blaz"
dist_dir    := "dist"

# Version from backend/Cargo.toml (evaluated once when the justfile is loaded)
version := `grep -m1 '^version\s*=' backend/Cargo.toml \
  | sed 's/.*"\(.*\)".*/\1/' \
  | tr -d '\n'`

tag := "v" + version

# Precomputed artifact paths (string concatenation, no {{}} here)
dist_backend := dist_dir + "/" + binary_name + "-" + version + "-linux-x86_64"
dist_apk     := dist_dir + "/" + binary_name + "-" + version + "-android-arm64.apk"

# ---- Recipes ----

# Build everything by default
default: build

build: build-backend build-apk
    @echo "Built backend and APK into {{dist_dir}}/"
    @ls -lh {{dist_dir}}

# Build Rust backend (release, host x86_64)
build-backend:
    @echo "Building backend (version {{version}})..."
    cd backend && cargo build --release
    mkdir -p {{dist_dir}}
    cp backend/target/release/{{binary_name}} {{dist_backend}}
    @echo "Backend → {{dist_backend}}"

# Build Android APK (arm64)
build-apk:
    @echo "Building Flutter APK (version {{version}})..."
    cd flutter && flutter build apk --release --target-platform android-arm64
    mkdir -p {{dist_dir}}
    cp flutter/build/app/outputs/flutter-apk/app-release.apk {{dist_apk}}
    @echo "APK → {{dist_apk}}"

# Create a GitHub release with both artifacts
# Requires: gh CLI installed & authenticated (`gh auth login`)
release: build-backend build-apk
    @echo "Creating GitHub release {{tag}} with:"
    @echo "  - {{dist_backend}}"
    @echo "  - {{dist_apk}}"
    gh release create {{tag}} \
        {{dist_backend}} \
        {{dist_apk}} \
        --title "{{tag}}" \
        --notes "Release {{tag}}"

