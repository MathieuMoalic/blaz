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
        echo "  2. Run: just update-prebuilt $new_version"
        echo "  3. Commit and push the flake.nix update"
    else
        echo "⚠ Skipped push. To push later:"
        echo "  git push && git push --tags"
    fi

# Update prebuilt package hash after a release is published
update-prebuilt VERSION:
    #!/usr/bin/env bash
    set -euo pipefail
    
    echo "📦 Updating prebuilt package to v{{VERSION}}..."
    
    # Wait for release to be available
    URL="https://github.com/MathieuMoalic/blaz/releases/download/v{{VERSION}}/blaz-v{{VERSION}}-x86_64-linux"
    echo "Checking if release is available..."
    
    if ! curl --fail --silent --head "$URL" > /dev/null; then
        echo "❌ Release not found at: $URL"
        echo "Make sure GitHub Actions has completed and the release is published."
        exit 1
    fi
    
    echo "✓ Release found"
    echo "Fetching hash..."
    
    # Fetch the hash
    HASH=$(nix-prefetch-url "$URL" 2>/dev/null)
    SRI_HASH=$(nix hash convert --to sri "sha256:$HASH")
    
    echo "✓ Hash: $SRI_HASH"
    echo ""
    
    # Update flake.nix
    sed -i "s/version = \"[0-9.]*\";  # Update AFTER/version = \"{{VERSION}}\";  # Update AFTER/" flake.nix
    sed -i "s/sha256 = \"sha256-[^\"]*\";  # Update with/sha256 = \"${SRI_HASH}\";  # Update with/" flake.nix
    
    echo "✓ Updated flake.nix"
    echo ""
    
    # Show diff
    echo "📝 Changes:"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    git diff flake.nix
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    
    # Test the build
    read -p "Test the prebuilt package? (Y/n) " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Nn]$ ]]; then
        echo "Testing build..."
        nix build .#prebuilt
        ./result/bin/blaz --version
        echo "✓ Build successful"
    fi
    
    echo ""
    read -p "Commit and push? (y/N) " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        git add flake.nix
        git commit -m "Update prebuilt package to v{{VERSION}}"
        git push
        echo "✓ Pushed update"
    else
        echo "⚠ Changes staged but not committed. Commit with:"
        echo "  git add flake.nix && git commit -m 'Update prebuilt to v{{VERSION}}' && git push"
    fi
