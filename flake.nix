{
  description = "Android app dev shell + Rust (musl) backend (blaz)";

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

      android = pkgs.androidenv.composeAndroidPackages {
        platformVersions = ["35" "34"];
        buildToolsVersions = ["35.0.0" "34.0.0"];
        includeEmulator = false;
      };
      sdkRoot = "${android.androidsdk}/libexec/android-sdk";

      muslTarget =
        {
          x86_64-linux = "x86_64-unknown-linux-musl";
          aarch64-linux = "aarch64-unknown-linux-musl";
        }.${
          system
        };

      rustSrc = ./backend;
      rustLockfile = ./backend/Cargo.lock;
    in rec {
      packages.default = pkgs.pkgsStatic.rustPlatform.buildRustPackage {
        pname = "blaz";
        version = "0.1.0";
        src = rustSrc;
        cargoLock.lockFile = rustLockfile;

        # Ensure cargo targets musl; some crates need crt-static to fully static link.
        CARGO_BUILD_TARGET = muslTarget;
        RUSTFLAGS = "-C target-feature=+crt-static";

        cargoBuildFlags = ["--locked"];
        doCheck = false;
      };

      apps.default = {
        type = "app";
        program = "${packages.blaz}/bin/blaz";
      };

      devShells.default = pkgs.mkShell {
        name = "android-dev-shell";
        buildInputs = with pkgs; [
          jdk17
          gradle
          android.androidsdk
          just
          rustc
          cargo
          clippy
          rustfmt
          pkg-config
          mold
        ];
        ANDROID_HOME = sdkRoot;
        ANDROID_SDK_ROOT = sdkRoot;
        JAVA_HOME = pkgs.jdk17;
        shellHook = ''
          export PATH="$ANDROID_SDK_ROOT/platform-tools:$ANDROID_SDK_ROOT/cmdline-tools/latest/bin:$PATH"
           export RUSTFLAGS="-C link-arg=-fuse-ld=mold"
        '';
      };
    });
}
