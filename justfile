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

update-server:
    ssh homeserver "cd /home/mat/nix; nix flake update blaz; up"
