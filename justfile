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
    else
        echo "⚠ Skipped push. To push later:"
        echo "  git push && git push --tags"
    fi
