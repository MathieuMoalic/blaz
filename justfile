# Bump version (major, minor, or patch)
bump TYPE:
    #!/usr/bin/env bash
    set -euo pipefail
    
    # Get current version from Cargo.toml
    current=$(grep '^version = ' backend/Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
    
    # Parse major.minor.patch
    IFS='.' read -r major minor patch <<< "$current"
    
    # Increment based on type
    case "{{TYPE}}" in
        major)
            major=$((major + 1))
            minor=0
            patch=0
            ;;
        minor)
            minor=$((minor + 1))
            patch=0
            ;;
        patch)
            patch=$((patch + 1))
            ;;
        *)
            echo "Error: TYPE must be major, minor, or patch"
            exit 1
            ;;
    esac
    
    new_version="$major.$minor.$patch"
    echo "Bumping version: $current → $new_version"
    
    # Update backend/Cargo.toml
    sed -i "s/^version = \"$current\"/version = \"$new_version\"/" backend/Cargo.toml
    
    # Update flutter/pubspec.yaml
    sed -i "s/^version: $current$/version: $new_version/" flutter/pubspec.yaml
    
    # Update Cargo.lock
    (cd backend && cargo check --quiet)
    
    # Stage changes and show diff
    git add backend/Cargo.toml backend/Cargo.lock flutter/pubspec.yaml
    echo ""
    echo "📝 Changes to be committed:"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    git diff --cached
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    
    # Commit and tag
    git commit -m "Bump version to $new_version"
    git tag -a "v$new_version" -m "Release v$new_version"

    echo "✓ Version bumped to $new_version"
    echo "✓ Committed and tagged as v$new_version"
    echo ""
    
    # Interactive push
    read -p "Push to remote? (y/N) " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        git push && git push --tags
        echo "✓ Pushed to remote"
        echo ""
        echo "🔄 Next steps:"
        echo "  1. Wait for GitHub Actions to build and release"
        echo "  2. Run: just local-release <type>"
        echo "  3. Commit and push the flake.nix update"
    else
        echo "⚠ Skipped push. To push later:"
        echo "  git push && git push --tags"
    fi

# Build and create release
release TYPE:
    #!/usr/bin/env bash
    set -euo pipefail
    
    TYPE="{{TYPE}}"
    VERSION=$(date +%Y.%m.%d)
    
    echo "📦 Building release: v${VERSION}-${TYPE}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    
    # Update version in manifests
    OLD_VERSION=$(grep '^version = ' backend/Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
    echo "Backend version: $OLD_VERSION"
    echo "Updating to: v${VERSION}-${TYPE}"
    
    # Update backend/Cargo.toml
    sed -i "s/^version = \"$OLD_VERSION\"/version = \"v${VERSION}-${TYPE}\"/" backend/Cargo.toml
    
    # Update backend/Cargo.lock
    (cd backend && cargo check --quiet)
    
    # Update flutter/pubspec.yaml
    sed -i "s/^version: $OLD_VERSION$/version: \"v${VERSION}-${TYPE}\"/" flutter/pubspec.yaml
    
    # Commit version bumps
    git add backend/Cargo.toml backend/Cargo.lock flutter/pubspec.yaml
    git commit -m "Bump version to v${VERSION}-${TYPE}"
    git tag -a "v${VERSION}-${TYPE}" -m "Release v${VERSION}-${TYPE}"
    
    echo ""
    echo "✓ Version bumped to v${VERSION}-${TYPE}"
    echo "✓ Committed and tagged"
    echo ""
    
    # Push changes and tags
    read -p "Push to remote? (y/N) " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        git push && git push --tags
        echo ""
        echo "🔄 Release triggered!"
        echo "  - GitHub Actions will build backend (Rust)"
        echo "  - GitHub Actions will build Flutter apps"
        echo "  - Releases will be created automatically"
    else
        echo "⚠ Changes committed and tagged but not pushed."
        echo "To push later:"
        echo "  git push && git push --tags"
    fi

# Update server flake
update-server:
    #!/usr/bin/env bash
    set -euo pipefail
    
    echo "📦 Updating server flake..."
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    echo "This will update the Blaz flake on the server."
    echo "Make sure you're connected to homeserver."
    echo ""
    
    read -p "Continue? (y/N) " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        ssh homeserver "cd /home/mat/nix; nix flake update blaz; up"
        echo ""
        echo "✓ Server flake updated"
    else
        echo "⚠ Skipped."
    fi

# Local release (builds everything locally and creates GitHub releases)
local-release TYPE:
    #!/usr/bin/env bash
    set -euo pipefail
    
    TYPE="{{TYPE}}"
    
    echo "📦 Local Release Process"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    echo "Type: ${TYPE}"
    echo ""
    
    # Check if release.py exists
    if [ ! -f "scripts/release.py" ]; then
        echo "❌ Error: scripts/release.py not found"
        echo "Please run: python3 scripts/release.py release ${TYPE}"
        exit 1
    fi
    
    echo "Running release.py..."
    python3 scripts/release.py release "${TYPE}"

start-copilot:
    #!/usr/bin/env bash
    mkdir -p ~/.local/copilot-shims
    ln -sf /run/current-system/sw/bin/bash ~/.local/copilot-shims/bash
    export PATH="$HOME/.local/copilot-shims:$PATH"
    export SHELL=/run/current-system/sw/bin/bash
    export CONFIG_SHELL=/run/current-system/sw/bin/bash
    exec copilot --allow-all
