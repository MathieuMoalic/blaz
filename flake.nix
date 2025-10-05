{
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
          android_sdk.accept_license = true;
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
    in {
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
          export RUSTFLAGS="-C link-arg=-fuse-ld=mold"

          cat > "flutter/android/local.properties" <<EOF
                sdk.dir=${sdkRoot}
                cmake.dir=${sdkRoot}/cmake/3.22.1
                ndk.dir=${sdkRoot}/ndk/27.0.12077973
                flutter.buildMode=debug
                flutter.versionName=1.0.0
                flutter.versionCode=1
          EOF
        '';
      };
    });
}
