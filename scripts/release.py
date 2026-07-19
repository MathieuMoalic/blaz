#!/usr/bin/env python3
"""
Release script for Blaz project.
Builds both backend (Rust) and frontend (Flutter), creates releases, and updates flake.nix.
"""

import subprocess
import sys
import tempfile
import shutil
from pathlib import Path
from datetime import datetime

def run_command(cmd, cwd=None, capture_output=True, check=True):
    """Run a shell command and return the result."""
    print(f"Running: {' '.join(cmd)}")
    result = subprocess.run(
        cmd,
        cwd=cwd,
        capture_output=capture_output,
        text=True,
        check=check,
    )
    if result.stdout:
        print(result.stdout)
    if result.stderr:
        print(result.stderr, file=sys.stderr)
    return result

def build_backend():
    """Build the backend in release mode."""
    print("\n=== Building Backend ===")
    run_command(["cargo", "build", "--release"], cwd="backend")
    print("Backend build complete")

def build_flutter():
    """Build the Flutter app in release mode."""
    print("\n=== Building Flutter ===")
    # First, update dependencies and analyze
    run_command(["flutter", "pub", "get"], cwd="flutter")
    run_command(["flutter", "analyze"], cwd="flutter")
    
    # Build for all platforms
    platforms = [
        "apk",    # Android
        "appbundle",  # Android App Bundle
        "ios",    # iOS (macOS only)
        "macos",  # macOS
        "linux",  # Linux
        "windows",   # Windows
    ]
    
    for platform in platforms:
        print(f"\nBuilding for {platform}...")
        try:
            run_command(
                ["flutter", "build", platform, "--release"],
                cwd="flutter"
            )
        except subprocess.CalledProcessError:
            print(f"Skipping {platform} (not available on this system)")
    
    print("Flutter build complete")

def create_release_notes(version):
    """Generate release notes."""
    release_notes = f"""# Release {version}

## What's Changed

This release includes the following updates:

- Simplified Android notifications to send one notification per recipe on the day before cooking at 20:00
- Removed multiple reminder notifications at 2h, 12h, and 24h
- Notifications now group by recipe and are sent only once per day

## Built Artifacts

### Backend
- Backend binary in `backend/target/release/blaz`

### Flutter
- Android APK: `flutter/build/app/outputs/flutter-apk/app-release.apk`
- Android App Bundle: `flutter/build/app/outputs/bundle/release/app-release.aab`
- Platform packages:
  - flutter/build/linux/x64/release/bundle/
  - flutter/build/macos/Build/Products/Release/
  - flutter/build/ios/Archive/*
  - flutter/build/windows/x64/release/bundle/

## Installation

### Android
Install the APK from the release assets.

### Other Platforms
Install from your respective app store or use the bundled binaries directly.
"""
    return release_notes

def create_github_release(tag, notes):
    """Create a GitHub release with the given tag and notes."""
    print("\n=== Creating GitHub Release ===")
    
    # Get the repo owner and name from git config
    result = run_command(["git", "config", "--get", "remote.origin.url"], capture_output=False, check=False)
    if result.returncode != 0:
        print("Error: Unable to determine repository URL from git config")
        sys.exit(1)
    
    # Extract owner/repo from URL (supports ssh and https formats)
    url = result.stdout.strip()
    if "://" in url:
        parts = url.rstrip("/").split("/")
        if len(parts) >= 4:
            owner = parts[-2]
            repo = parts[-1]
        else:
            print(f"Error: Could not parse repository URL: {url}")
            sys.exit(1)
    else:
        # SSH format: git@github.com:owner/repo.git
        parts = url.rstrip(".git").split(":")
        owner, repo = parts[-2], parts[-1]
    
    # Use GITHUB_TOKEN environment variable if available
    token = "GITHUB_TOKEN"
    
    # Create release
    release_cmd = [
        "gh", "release", "create", tag,
        "--title", f"Release {tag}",
        "--notes", notes,
        "--repo", f"{owner}/{repo}",
    ]
    
    result = run_command(release_cmd)
    
    if result.returncode != 0:
        print(f"Error: Failed to create GitHub release for {tag}")
        sys.exit(1)
    
    print(f"Created GitHub release {tag}")

def update_flake_nix():
    """Update the flake.nix hash for both backend and flutter."""
    print("\n=== Updating flake.nix ===")
    
    # Update backend hash
    backend_build_path = Path("backend/target/release/blaz")
    if backend_build_path.exists():
        with open("flake.nix", "r") as f:
            flake_content = f.read()
        
        # Calculate new hash
        new_hash = run_command(
            ["nix", "hash", "to-base32", "--type", "sha256", 
             str(backend_build_path.absolute())],
            capture_output=False
        ).stdout.strip()
        
        # Replace backend hash in flake.nix
        # Looking for the app section and updating the x86_64-linux hash
        flake_lines = flake_content.split("\n")
        in_backend_section = False
        new_lines = []
        
        for i, line in enumerate(flake_lines):
            if "'blaz'" in line or '"blaz"' in line:
                in_backend_section = True
            
            if in_backend_section and ("sha256 = " in line or 'sha256 = "' in line):
                # Find the hash line (skip first hash in the expression)
                if i > 0 and "'blaz'" not in flake_lines[i-1]:
                    new_lines.append(f"        sha256 = \"{new_hash}\";")
                    # Skip the rest of this hash line
                    while i < len(flake_lines) and not flake_lines[i].strip().endswith(";"):
                        i += 1
                    continue
            
            new_lines.append(line)
        
        with open("flake.nix", "w") as f:
            f.write("\n".join(new_lines))
        
        print("Updated backend hash in flake.nix")
    
    # Update flutter hash
    flutter_build_path = Path("flutter/build/app/outputs/flutter-apk/app-release.apk")
    if flutter_build_path.exists():
        with open("flake.nix", "r") as f:
            flake_content = f.read()
        
        # Calculate new hash
        new_hash = run_command(
            ["nix", "hash", "to-base32", "--type", "sha256",
             str(flutter_build_path.absolute())],
            capture_output=False
        ).stdout.strip()
        
        # Replace flutter hash in flake.nix
        flake_lines = flake_content.split("\n")
        in_flutter_section = False
        new_lines = []
        
        for i, line in enumerate(flake_lines):
            if "blaz-flutter" in line or "blaz_flutter" in line:
                in_flutter_section = True
            
            if in_flutter_section and ("sha256 = " in line or 'sha256 = "' in line):
                # Find the hash line
                if i > 0 and ("blaz-flutter" not in flake_lines[i-1] and 
                             "blaz_flutter" not in flake_lines[i-1]):
                    new_lines.append(f"        sha256 = \"{new_hash}\";")
                    # Skip the rest of this hash line
                    while i < len(flake_lines) and not flake_lines[i].strip().endswith(";"):
                        i += 1
                    continue
            
            new_lines.append(line)
        
        with open("flake.nix", "w") as f:
            f.write("\n".join(new_lines))
        
        print("Updated flutter hash in flake.nix")

def main():
    if len(sys.argv) < 2:
        print("Usage: python3 release.py release <type>")
        print("Example: python3 release.py release stable")
        sys.exit(1)
    
    release_type = sys.argv[1]
    
    # Get current date for version
    today = datetime.now().strftime("%Y.%m.%d")
    version = f"{today}-{release_type}"
    
    print(f"=== Starting Release Process for {version} ===")
    print(f"Release type: {release_type}")
    
    # Ensure we're on the main branch
    result = run_command(["git", "rev-parse", "--abbrev-ref", "HEAD"])
    if "main" not in result.stdout and "master" not in result.stdout:
        print("Warning: Not on main or master branch")
        response = input("Continue anyway? [y/N] ").strip().lower()
        if response != "y":
            sys.exit(1)
    
    # Create a new git tag
    print(f"\n=== Creating Tag ===")
    run_command(["git", "tag", "-a", version, "-m", f"Release {version}"])
    print(f"Created tag {version}")
    
    # Build everything
    build_backend()
    build_flutter()
    
    # Create release notes
    notes = create_release_notes(version)
    
    # Create GitHub release
    create_github_release(version, notes)
    
    # Update flake.nix
    update_flake_nix()
    
    # Commit the flake.nix update
    print("\n=== Committing Updates ===")
    run_command(["git", "add", "flake.nix"])
    run_command(["git", "commit", "-m", f"chore: update flake.nix hashes for {version}"])
    print(f"Committed flake.nix update")
    
    print(f"\n=== Release {version} Complete ===")
    print("Don't forget to push your tags:")
    print(f"  git push origin {version}")
    print("  git push origin main")

if __name__ == "__main__":
    main()
