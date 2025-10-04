{
  description = "Android app dev shell (adb + SDK) for NixOS";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs"; # you already pinned this commit
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = {
    nixpkgs,
    flake-utils,
    ...
  }:
    flake-utils.lib.eachDefaultSystem (system: let
      pkgs = import nixpkgs {
        inherit system;
        config = {
          allowUnfree = true;
          android_sdk.accept_license = true; # <-- this replaces licenseAccepted
        };
      };

      android = pkgs.androidenv.composeAndroidPackages {
        platformVersions = ["35" "34"];
        buildToolsVersions = ["35.0.0" "34.0.0"];
      };

      # Helpful absolute path into the SDK layout
      sdkRoot = "${android.androidsdk}/libexec/android-sdk";
    in {
      devShells.default = pkgs.mkShell {
        name = "android-dev-shell";

        buildInputs = [
          pkgs.jdk17
          pkgs.gradle
          android.androidsdk
        ];

        ANDROID_HOME = sdkRoot;
        ANDROID_SDK_ROOT = sdkRoot;
        JAVA_HOME = pkgs.jdk17;

        shellHook = ''
          export PATH="$ANDROID_SDK_ROOT/platform-tools:$ANDROID_SDK_ROOT/cmdline-tools/latest/bin:$PATH"
        '';
      };
    });
}
