{
  description = "Android app dev shell (adb + SDK) for NixOS";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs"; # you already pinned this commit
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
  }:
    flake-utils.lib.eachDefaultSystem (system: let
      pkgs = import nixpkgs {
        inherit system;
        config = {
          allowUnfree = true; # for Android Studio if you add it
          android_sdk.accept_license = true; # <-- this replaces licenseAccepted
        };
      };

      android = pkgs.androidenv.composeAndroidPackages {
        # Pick the SDKs you need. API 35 is Android 15.
        platformVersions = ["35" "34"];
        buildToolsVersions = ["35.0.0" "34.0.0"];
        includeEmulator = false; # set true if you want the emulator
        # For NDK, uncomment and choose a version available in nixpkgs:
        # ndkVersions = [ "26.3.11579264" ];
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
          # Optional IDE:
          # pkgs.android-studio
        ];

        ANDROID_HOME = sdkRoot;
        ANDROID_SDK_ROOT = sdkRoot;
        JAVA_HOME = pkgs.jdk17;

        shellHook = ''
          export PATH="$ANDROID_SDK_ROOT/platform-tools:$ANDROID_SDK_ROOT/cmdline-tools/latest/bin:$PATH"
          echo "SDK:    $ANDROID_SDK_ROOT"
          echo "Java:   $JAVA_HOME"
          echo "Try:    adb devices"
        '';
      };
    });
}
