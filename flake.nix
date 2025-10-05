{
  description = "Flutter dev shell + Rust (musl) backend (blaz)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = {
    nixpkgs,
    flake-utils,
    ...
  }:
    flake-utils.lib.eachSystem ["x86_64-linux" "aarch64-linux"] (system: let
      pkgs = import nixpkgs {
        inherit system;
        config = {
          allowUnfree = true;
          android_sdk.accept_license = true; # accept Android SDK licenses
        };
      };

      muslTarget =
        {
          x86_64-linux = "x86_64-unknown-linux-musl";
          aarch64-linux = "aarch64-unknown-linux-musl";
        }.${
          system
        };

      rustSrc = ./backend;
      rustLockfile = ./backend/Cargo.lock;

      androidSdk =
        (pkgs.androidenv.composeAndroidPackages {
          platformVersions = ["35" "34"];
          buildToolsVersions = ["35.0.0" "34.0.0"];
          ndkVersions = ["27.0.12077973"];
          includeNDK = true;
          includeEmulator = false;
          cmakeVersions = ["3.22.1"];
          includeCmake = true;
        }).androidsdk;

      sdkRoot = "${androidSdk}/libexec/android-sdk";
    in rec {
      packages.default = pkgs.pkgsStatic.rustPlatform.buildRustPackage {
        pname = "blaz";
        version = "0.1.0";
        src = rustSrc;
        cargoLock.lockFile = rustLockfile;

        CARGO_BUILD_TARGET = muslTarget;
        RUSTFLAGS = "-C target-feature=+crt-static";

        cargoBuildFlags = ["--locked"];
        doCheck = false;
      };

      devShells.default = pkgs.mkShell {
        name = "dev-shell";
        packages = with pkgs; [
          flutter
          dart
          androidSdk
          android-tools
          jdk17
          just

          # Rust
          rustc
          cargo
          clippy
          rustfmt
          pkg-config
          mold
        ];

        shellHook = ''
          # Android tooling
          export ANDROID_SDK_ROOT="${sdkRoot}"
          export ANDROID_HOME="${sdkRoot}"
          export ANDROID_NDK_HOME="${sdkRoot}/ndk/27.0.12077973"
          export ANDROID_NDK_ROOT="${sdkRoot}/ndk/27.0.12077973"
          export JAVA_HOME="${pkgs.jdk17}"
          export PATH="$ANDROID_SDK_ROOT/platform-tools:$ANDROID_SDK_ROOT/cmdline-tools/latest/bin:$PATH"
          export RUSTFLAGS="-C link-arg=-fuse-ld=mold"
        '';
      };

      checks.build = packages.blaz;
      formatter = pkgs.nixpkgs-fmt;
    });
}
