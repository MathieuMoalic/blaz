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
    lib = pkgs.lib;

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

    package = pkgs.stdenvNoCC.mkDerivation rec {
      pname = "blaz-backend";
      version = "0.1.0";
      src = pkgs.fetchurl {
        url = "https://github.com/MathieuMoalic/blaz/releases/download/v${version}/blaz-${version}-linux-x86_64";
        hash = "sha256-dwfCCwPL3+a5R6rpr1zKZ3P2J8LJJgoa/Qls8UlzXFM=";
      };

      dontUnpack = true;

      installPhase = ''
        runHook preInstall
        install -Dm755 "$src" "$out/bin/blaz"
        runHook postInstall
      '';

      meta = with lib; {
        description = "blaz backend (prebuilt binary from GitHub release)";
        homepage = "https://github.com/${owner}/${repo}";
        license = licenses.mit; # or whatever you actually use
        maintainers = [];
        mainProgram = "blaz";
        platforms = ["x86_64-linux"];
      };
    };

    overlay = final: prev: {
      blaz-backend = package;
    };

    service = {
      lib,
      config,
      pkgs,
      ...
    }: let
      cfg = config.services.blaz;
    in {
      options.services.blaz = {
        enable = lib.mkEnableOption "blaz backend service";

        package = lib.mkOption {
          type = lib.types.package;
          default = package;
          description = "blaz backend package to run.";
        };

        # Generic env passed to the service (e.g. DB URL, secrets)
        environment = lib.mkOption {
          type = lib.types.attrsOf lib.types.str;
          default = {};
          description = "Environment variables passed to the blaz backend.";
        };

        port = lib.mkOption {
          type = lib.types.port;
          default = 8080;
          description = "Port the blaz backend listens on.";
        };

        user = lib.mkOption {
          type = lib.types.str;
          default = "blaz";
          description = "User account under which the blaz service runs.";
        };

        group = lib.mkOption {
          type = lib.types.str;
          default = "blaz";
          description = "Group under which the blaz service runs.";
        };
      };

      config = lib.mkIf cfg.enable {
        users.users.${cfg.user} = {
          isSystemUser = true;
          group = cfg.group;
          home = "/var/lib/blaz";
          createHome = true;
        };
        users.groups.${cfg.group} = {};

        systemd.services.blaz = {
          description = "blaz backend";
          after = ["network.target"];
          wantedBy = ["multi-user.target"];

          environment =
            cfg.environment
            // {
              PORT = builtins.toString cfg.port;
            };

          serviceConfig = {
            ExecStart = "${cfg.package}/bin/blaz";
            WorkingDirectory = "/var/lib/blaz";
            User = cfg.user;
            Group = cfg.group;
            StateDirectory = "blaz";

            # Security hardening (mirroring your boued setup)
            CapabilityBoundingSet = "";
            RestrictAddressFamilies = "AF_UNIX AF_INET AF_INET6";
            SystemCallFilter = "~@clock @cpu-emulation @keyring @module @obsolete @raw-io @reboot @swap @resources @privileged @mount @debug";
            NoNewPrivileges = "yes";
            ProtectClock = "yes";
            ProtectKernelLogs = "yes";
            ProtectControlGroups = "yes";
            ProtectKernelModules = "yes";
            SystemCallArchitectures = "native";
            RestrictNamespaces = "yes";
            RestrictSUIDSGID = "yes";
            ProtectHostname = "yes";
            ProtectKernelTunables = "yes";
            RestrictRealtime = "yes";
            ProtectProc = "invisible";
            PrivateUsers = "yes";
            LockPersonality = "yes";
            UMask = "0077";
            RemoveIPC = "yes";
            LimitCORE = "0";
            ProtectHome = "yes";
            PrivateTmp = "yes";
            ProtectSystem = "strict";
            ProcSubset = "pid";
            SocketBindAllow = ["tcp:${builtins.toString cfg.port}"];
            SocketBindDeny = "any";

            LimitNOFILE = 1024;
            LimitNPROC = 64;
            MemoryMax = "200M";
          };
        };
      };
    };
    app = {
      type = "app";
      program = lib.getExe package;
    };
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
    packages.${system}.default = package;
    apps.${system}.default = app;
    devShells.${system}.default = shell;
    overlays.default = overlay;
    nixosModules.blaz-service = service;
  };
}
