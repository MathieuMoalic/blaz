#!/usr/bin/env python3

from __future__ import annotations

import argparse
import re
import shutil
import subprocess
import sys
import tarfile
from pathlib import Path


APP = "blaz"
TARGET = "x86_64-linux"
REPO = "MathieuMoalic/blaz"

ROOT = Path(__file__).resolve().parents[1]
BACKEND = ROOT / "backend"
FLUTTER = ROOT / "flutter"
FLAKE = ROOT / "flake.nix"
RELEASE_DIR = ROOT / "release" / "artifacts"

CARGO_TOML = BACKEND / "Cargo.toml"
CARGO_LOCK = BACKEND / "Cargo.lock"
PUBSPEC = FLUTTER / "pubspec.yaml"


def run(*cmd: str, cwd: Path | None = None) -> None:
    print("+", " ".join(cmd))
    subprocess.run(cmd, cwd=cwd or ROOT, check=True)


def output(*cmd: str, cwd: Path | None = None) -> str:
    return subprocess.check_output(cmd, cwd=cwd or ROOT, text=True).strip()


def ensure_clean_tree() -> None:
    status = output("git", "status", "--short")
    if status:
        print("Error: working tree is dirty. Commit or stash changes before releasing.")
        print(status)
        sys.exit(1)


def current_version() -> str:
    text = CARGO_TOML.read_text()
    match = re.search(r'(?m)^version = "([0-9]+\.[0-9]+\.[0-9]+)"$', text)
    if not match:
        raise RuntimeError("Could not find version in backend/Cargo.toml")
    return match.group(1)


def bump_version(version: str, bump_type: str) -> str:
    major, minor, patch = map(int, version.split("."))

    match bump_type:
        case "major":
            return f"{major + 1}.0.0"
        case "minor":
            return f"{major}.{minor + 1}.0"
        case "patch":
            return f"{major}.{minor}.{patch + 1}"
        case _:
            raise RuntimeError("TYPE must be major, minor, or patch")


def replace_once(text: str, pattern: str, replacement: str, label: str) -> str:
    new_text, count = re.subn(pattern, replacement, text, count=1)
    if count != 1:
        raise RuntimeError(f"Failed to update {label}")
    return new_text


def replace_all_existing(
    text: str,
    pattern: str,
    replacement: str,
    label: str,
) -> str:
    new_text, count = re.subn(pattern, replacement, text)
    if count == 0:
        raise RuntimeError(f"Failed to update {label}")
    return new_text


def update_version_files(old: str, new: str) -> None:
    print(f"Bumping version: {old} -> {new}")

    cargo = CARGO_TOML.read_text()
    cargo = replace_once(
        cargo,
        rf'(?m)^version = "{re.escape(old)}"$',
        f'version = "{new}"',
        "backend/Cargo.toml version",
    )
    CARGO_TOML.write_text(cargo)

    pubspec = PUBSPEC.read_text()
    pubspec = replace_once(
        pubspec,
        rf'(?m)^version:\s*"?{re.escape(old)}"?(?:\+\d+)?\s*$',
        f"version: {new}",
        "flutter/pubspec.yaml version",
    )
    PUBSPEC.write_text(pubspec)


def update_flake_versions(version: str) -> None:
    text = FLAKE.read_text()

    text = replace_all_existing(
        text,
        r'(pname = "blaz-web";\n\s+version = ")[^"]+(";)',
        rf"\g<1>{version}\2",
        "flake.nix blaz-web version",
    )

    text = replace_all_existing(
        text,
        r'(pname = "blaz";\n\s+version = ")[^"]+(";)',
        rf"\g<1>{version}\2",
        "flake.nix blaz versions",
    )

    FLAKE.write_text(text)


def update_flake_flutter(version: str, apk_hash: str) -> None:
    """Update flake.nix for Flutter web build and prebuilt package."""
    text = FLAKE.read_text()
    
    # Update web build version
    lines = text.split("\n")
    web_build_updated = False
    
    i = 0
    while i < len(lines):
        line = lines[i]
        if ('webBuild = pkgs.flutter.buildFlutterApplication {' in line and 
            i + 2 < len(lines) and 
            'pname = "blaz-web";' in lines[i + 1] and 
            'version = "0.1.0";' in lines[i + 2]):
            # Update the version line
            lines[i] = line
            lines[i + 1] = lines[i + 1]
            lines[i + 2] = line.replace("0.1.0", version)
            web_build_updated = True
            i += 3
        else:
            i += 1
    
    if not web_build_updated:
        raise RuntimeError("Failed to update web build version")
    
    # Update prebuilt package version and src to local
    text = "\n".join(lines)
    
    lines = text.split("\n")
    in_prebuilt_section = False
    src_replaced = False
    version_updated = False
    
    i = 0
    while i < len(lines):
        line = lines[i]
        if in_prebuilt_section:
            if 'version = "2.8.1";' in line and not version_updated:
                # Update version
                lines[i] = line.replace('version = "2.8.1";', f'version = "{version}";')
                version_updated = True
                i += 1
                continue
            elif 'src = pkgs.fetchurl {' in line and not src_replaced:
                # Replace with local src and advance past the fetchurl block
                brace_count = 1
                j = i + 1
                while j < len(lines) and brace_count > 0:
                    if '{' in lines[j]:
                        brace_count += 1
                    if '}' in lines[j]:
                        brace_count -= 1
                    j += 1
                
                # Replace the src line and skip to end of fetchurl block
                lines[i] = line.replace('src = pkgs.fetchurl {', 'src = ./backend;')
                i = j
                in_prebuilt_section = False
                src_replaced = True
                continue
            else:
                i += 1
        else:
            if "prebuiltPackage = pkgs.stdenvNoCC.mkDerivation rec {" in line:
                in_prebuilt_section = True
            i += 1
    
    text = "\n".join(lines)
    
    if not version_updated:
        raise RuntimeError("Failed to update prebuilt package version")
    if not src_replaced:
        raise RuntimeError("Failed to update prebuilt package src")
    
    FLAKE.write_text(text)


def update_flake_prebuilt(version: str, nix_hash: str) -> None:
    """Update flake.nix for local builds."""
    text = FLAKE.read_text()
    
    # Update web build version
    lines = text.split("\n")
    web_build_updated = False
    
    i = 0
    while i < len(lines):
        line = lines[i]
        if ('webBuild = pkgs.flutter.buildFlutterApplication {' in line and 
            i + 2 < len(lines) and 
            'pname = "blaz-web";' in lines[i + 1] and 
            'version = "0.1.0";' in lines[i + 2]):
            # Update the version line
            lines[i + 2] = lines[i + 2].replace("0.1.0", version)
            web_build_updated = True
            i += 3
        else:
            i += 1
    
    if not web_build_updated:
        raise RuntimeError("Failed to update web build version")
    
    # Replace prebuiltPackage with just a reference to package
    text = "\n".join(lines)
    
    # Find the prebuiltPackage section and replace it with just the assignment
    lines = text.split("\n")
    output_lines = []
    skip_until_closing = False
    prebuilt_found = False
    brace_count = 0
    
    for i, line in enumerate(lines):
        if 'prebuiltPackage = pkgs.stdenvNoCC.mkDerivation rec {' in line:
            skip_until_closing = True
            brace_count = 1
            prebuilt_found = True
            output_lines.append('    prebuilt = package;')
            continue
        
        if skip_until_closing:
            # Count braces to find the end of the prebuiltPackage block
            if '{' in line:
                brace_count += 1
            if '}' in line:
                brace_count -= 1
            
            if brace_count == 0:
                skip_until_closing = False
            continue
        
        output_lines.append(line)
    
    if not prebuilt_found:
        raise RuntimeError("Failed to find prebuiltPackage in flake.nix")
    
    # Also update the outputs section to reference package instead of prebuiltPackage
    text = "\n".join(output_lines)
    text = text.replace('prebuilt = prebuiltPackage;', 'prebuilt = package;')
    
    FLAKE.write_text(text)


def cargo_check() -> None:
    run("cargo", "check", "--quiet", cwd=BACKEND)


def build_flutter_web() -> None:
    web_build = BACKEND / "web_build"
    flutter_build_web = FLUTTER / "build" / "web"

    run("flutter", "pub", "get", cwd=FLUTTER)
    run("flutter", "build", "web", "--release", cwd=FLUTTER)

    if web_build.exists():
        shutil.rmtree(web_build)
    shutil.copytree(flutter_build_web, web_build)


def build_backend_archive(version: str) -> Path:
    tag = f"v{version}"
    binary_name = f"{APP}-{tag}-{TARGET}"
    archive_name = f"{binary_name}.tar.gz"
    binary_path = RELEASE_DIR / binary_name
    archive_path = RELEASE_DIR / archive_name
    out_link = ROOT / ".release-backend"

    run("nix", "build", ".#backend", "--out-link", str(out_link))
    source = out_link / "bin" / APP
    shutil.copy2(source, binary_path)
    binary_path.chmod(0o755)

    with tarfile.open(archive_path, "w:gz") as tar:
        tar.add(binary_path, arcname=binary_name)

    binary_path.unlink()
    if out_link.exists() or out_link.is_symlink():
        out_link.unlink()
    return archive_path


def build_apk(version: str) -> Path:
    tag = f"v{version}"
    artifact = RELEASE_DIR / f"{APP}-{tag}.apk"

    run("flutter", "pub", "get", cwd=FLUTTER)
    build_number = output("git", "rev-list", "--count", "HEAD")

    run(
        "flutter",
        "build",
        "apk",
        "--release",
        "--build-name",
        version,
        "--build-number",
        build_number,
        cwd=FLUTTER,
    )

    source = (
        FLUTTER / "build" / "app" / "outputs" / "flutter-apk" / "app-release.apk"
    )
    shutil.copy2(source, artifact)
    return artifact


def nix_hash_file(path: Path) -> str:
    return output("nix", "hash", "file", "--type", "sha256", str(path))


def commit_and_tag(version: str) -> None:
    tag = f"v{version}"
    run("git", "add", str(CARGO_TOML), str(CARGO_LOCK), str(PUBSPEC), str(FLAKE))
    run("git", "--no-pager", "diff", "--cached", "--stat")
    run("git", "commit", "-m", f"Release {tag}")
    run("git", "tag", "-a", tag, "-m", f"Release {tag}")


def push_release_command(tag: str) -> None:
    if not re.fullmatch(r"v[0-9]+\.[0-9]+\.[0-9]+", tag):
        raise RuntimeError("TAG must look like v1.2.3")

    backend_artifact = RELEASE_DIR / f"{APP}-{tag}-{TARGET}.tar.gz"
    apk_artifact = RELEASE_DIR / f"{APP}-{tag}.apk"

    if not backend_artifact.exists():
        raise RuntimeError(f"Missing backend artifact: {backend_artifact}")
    if not apk_artifact.exists():
        raise RuntimeError(f"Missing APK artifact: {apk_artifact}")

    run(
        "gh",
        "release",
        "create",
        tag,
        "--generate-notes",
        "--",
        str(backend_artifact),
        str(apk_artifact),
    )


def bump_command(bump_type: str) -> None:
    old = current_version()
    new = bump_version(old, bump_type)
    update_version_files(old, new)
    update_flake_versions(new)
    cargo_check()
    print(f"Version files updated to {new}")


def release_command(bump_type: str) -> None:
    ensure_clean_tree()

    old = current_version()
    new = bump_version(old, bump_type)
    tag = f"v{new}"
    start_head = output("git", "rev-parse", "HEAD")
    pushed = False

    try:
        RELEASE_DIR.mkdir(parents=True, exist_ok=True)
        for item in RELEASE_DIR.iterdir():
            if item.is_dir():
                shutil.rmtree(item)
            else:
                item.unlink()

        update_version_files(old, new)
        cargo_check()
        build_flutter_web()
        
        # Update flake.nix before building backend
        update_flake_prebuilt(new, "placeholder")
        commit_and_tag(new)
        
        backend_artifact = build_backend_archive(new)
        nix_hash = nix_hash_file(backend_artifact)
        
        # Build Flutter APK and get hash
        apk_artifact = build_apk(new)
        apk_hash = nix_hash_file(apk_artifact)
        
        # Update flake.nix for Flutter
        update_flake_flutter(new, apk_hash)

        print("\nRelease artifacts:")
        print(f"  {backend_artifact}")
        print(f"  {apk_artifact}")

        pushed = True
        run("git", "push", "origin", "HEAD")
        run("git", "push", "origin", tag)
        push_release_command(tag)
        print(f"\nReleased {tag}")
    except Exception:
        if not pushed:
            run("git", "reset", "--hard", start_head)
            if output("git", "tag", "-l", tag) == tag:
                run("git", "tag", "-d", tag)
            if RELEASE_DIR.exists():
                shutil.rmtree(RELEASE_DIR)
            web_build = BACKEND / "web_build"
            if web_build.exists():
                shutil.rmtree(web_build)
        raise


def main() -> None:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)

    bump_parser = subparsers.add_parser("bump")
    bump_parser.add_argument("type", choices=["major", "minor", "patch"])

    release_parser = subparsers.add_parser("release")
    release_parser.add_argument("type", choices=["major", "minor", "patch"])

    push_parser = subparsers.add_parser("push-release")
    push_parser.add_argument("tag")

    args = parser.parse_args()

    try:
        match args.command:
            case "bump":
                bump_command(args.type)
            case "release":
                release_command(args.type)
            case "push-release":
                push_release_command(args.tag)
            case _:
                raise RuntimeError(f"Unknown command: {args.command}")
    except Exception as exc:
        print(f"Error: {exc}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
