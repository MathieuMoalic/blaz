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
        platformVersions = ["35" "34"];
        buildToolsVersions = ["35.0.0" "34.0.0"];

        ndkVersions = ["27.0.12077973"];
        includeNDK = true;

        cmakeVersions = ["3.22.1"];
        includeCmake = true;

        includeEmulator = false;
      }).androidsdk;

    sdkRoot = "${androidSdk}/libexec/android-sdk";

    shell = pkgs.mkShell {
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
      ANDROID_SDK_ROOT = sdkRoot;
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
  in {
    devShells.${system}.default = shell;
  };
}
