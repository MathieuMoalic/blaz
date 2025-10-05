{
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";

  outputs = {nixpkgs, ...}: let
    system = "x86_64-linux";
    pkgs = import nixpkgs {
      inherit system;
      config = {
        allowUnfree = true;
        android_sdk.accept_license = true;
      };
    };

    androidSdk =
      (pkgs.androidenv.composeAndroidPackages {
        platformVersions = ["35"];
        buildToolsVersions = ["35.0.0"];

        ndkVersions = ["27.0.12077973"];
        includeNDK = true;

        cmakeVersions = ["3.22.1"];
        includeCmake = true;

        includeEmulator = false;
      }).androidsdk;

    sdkRoot = "${androidSdk}/libexec/android-sdk";
  in {
    packages.${system}.default = pkgs.pkgsStatic.rustPlatform.buildRustPackage {
      pname = "blaz";
      version = "0.1.0";
      src = ./backend;
      cargoLock.lockFile = ./backend/Cargo.lock;
      CARGO_BUILD_TARGET = "x86_64-unknown-linux-musl";
      RUSTFLAGS = "-C target-feature=+crt-static";
      cargoBuildFlags = ["--locked"];
      doCheck = false;
    };

    devShells.${system}.default = pkgs.mkShell {
      name = "dev-shell";
      packages = with pkgs; [
        flutter
        dart
        androidSdk
        android-tools
        jdk17
        just

        rustc
        cargo
        clippy
        rustfmt
        pkg-config
        mold
        cargo-watch
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
  };
}
