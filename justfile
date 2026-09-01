
# Sync the dev database from the homeserver
backend-cp-db:
    trash -vrf -r $BLAZ_DATABASE_PATH $BLAZ_LOG_FILE $BLAZ_MEDIA_DIR/
    scp homeserver:/var/lib/blaz/blaz.sqlite $BLAZ_DATABASE_PATH

# Build the Flutter web app and copy it into backend/web_build/
frontend-build-web:
    cd flutter && flutter build web --release
    rm -rf backend/web_build
    mkdir -p backend/web_build
    cp -r flutter/build/web/* backend/web_build/

# Build the backend with embedded web assets
backend-build: frontend-build-web
    cd backend && cargo build --release

# Run the backend with hot reload
backend-hot-reload:
    cd backend && cargo watch -q -c -w src -w Cargo.toml -x 'run -- -vv'

# Run backend clippy on file changes
backend-clippy-watch:
    cd backend && cargo watch -q -c -w src -w Cargo.toml -x clippy

# Run backend tests on file changes
backend-test-watch:
    cd backend && cargo watch -q -c -w src -w Cargo.toml -x test

# Backfill legacy ingredients to canonical Food identity (idempotent)
backend-backfill-ingredients:
    cd backend && cargo run -- backfill-ingredients

# Run the Flutter web app
frontend-web:
    cd flutter && flutter run -d web-server --web-hostname 127.0.0.1 --web-port 5173

# Run the Flutter Linux app
frontend-linux:
    cd flutter && flutter run -d Linux

# Run the Flutter Android app
frontend-android:
    adb reverse tcp:8080 tcp:8080
    cd flutter && flutter run -d CPH2465

# Run Flutter tests on file changes
frontend-test-watch:
    cd flutter && watchexec -e dart -c -w lib -w test -- flutter test --reporter compact

# Generate Flutter launcher icons
frontend-gen-icons:
    cd flutter && flutter pub run flutter_launcher_icons

# Build release artifacts, update flake.nix hash, commit, and tag
release TYPE:
    python3 scripts/release.py release "{{TYPE}}"
    just backend-update-server


# Update the deployed backend on the homeserver
backend-update-server:
    ssh homeserver "cd /home/mat/nix; nix flake update blaz; up"
