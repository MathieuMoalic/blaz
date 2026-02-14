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
        sqlite
      ];

      RUSTFLAGS = "-C link-arg=-fuse-ld=mold";

      ANDROID_SDK_ROOT = sdkRoot;
      ANDROID_HOME = sdkRoot;
      JAVA_HOME = "${pkgs.jdk17}/lib/openjdk";
      PUB_CACHE = ".pub-cache";
      GRADLE_USER_HOME = ".gradle";
    };

    package = pkgs.rustPlatform.buildRustPackage {
      pname = "blaz";
      version = "0.1.0";
      src = ./backend;

      cargoLock = {
        lockFile = ./backend/Cargo.lock;
      };

      nativeBuildInputs = with pkgs; [
        pkg-config
      ];

      buildInputs = with pkgs; [
        sqlite
        openssl
      ];

      doCheck = false;

      meta = with lib; {
        description = "Recipe manager backend";
        homepage = "https://github.com/MathieuMoalic/blaz";
        license = licenses.gpl3;
        maintainers = [];
      };
    };

    service = {
      lib,
      config,
      pkgs,
      utils,
      ...
    }: let
      cfg = config.services.blaz;
    in {
      options.services.blaz = {
        enable = lib.mkEnableOption "Blaz recipe manager backend";

        bindAddr = lib.mkOption {
          type = lib.types.str;
          default = "127.0.0.1:8080";
          description = "Address to bind the HTTP server to";
        };

        databasePath = lib.mkOption {
          type = lib.types.str;
          default = "/var/lib/blaz/blaz.sqlite";
          description = "Path to SQLite database file";
        };

        mediaDir = lib.mkOption {
          type = lib.types.str;
          default = "/var/lib/blaz/media";
          description = "Directory to store media files (recipe images)";
        };

        logFile = lib.mkOption {
          type = lib.types.str;
          default = "/var/log/blaz/blaz.log";
          description = "Path to log file";
        };

        corsOrigin = lib.mkOption {
          type = lib.types.nullOr lib.types.str;
          default = null;
          example = "https://blaz.yourdomain.com";
          description = "CORS allowed origin. If null, allows any origin";
        };

        verbosity = lib.mkOption {
          type = lib.types.int;
          default = 0;
          description = "Log verbosity level (-2 to 3, where 0 is info)";
        };
      };

      config = lib.mkIf cfg.enable {
        users.users.blaz = {
          isSystemUser = true;
          group = "blaz";
          home = "/var/lib/blaz";
          createHome = true;
        };
        users.groups.blaz = {};

        systemd.services.blaz = {
          description = "Blaz recipe manager backend";
          after = ["network.target"];
          wantedBy = ["multi-user.target"];

          environment =
            {
              BLAZ_BIND_ADDR = cfg.bindAddr;
              BLAZ_DATABASE_PATH = cfg.databasePath;
              BLAZ_MEDIA_DIR = cfg.mediaDir;
              BLAZ_LOG_FILE = cfg.logFile;
            }
            // lib.optionalAttrs (cfg.corsOrigin != null) {
              BLAZ_CORS_ORIGIN = cfg.corsOrigin;
            };

          serviceConfig = {
            ExecStart = utils.escapeSystemdExecArgs ([
                "${package}/bin/blaz"
              ]
              ++ lib.optionals (cfg.verbosity > 0) (lib.genList (_: "-v") cfg.verbosity)
              ++ lib.optionals (cfg.verbosity < 0) (lib.genList (_: "-q") (- cfg.verbosity)));

            WorkingDirectory = "/var/lib/blaz";
            User = "blaz";
            Group = "blaz";
            StateDirectory = "blaz";
            LogsDirectory = "blaz";

            RuntimeDirectory = "blaz";
            RuntimeDirectoryMode = "0750";

            Restart = "always";
            RestartSec = "5s";

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
            UMask = "0027";
            RemoveIPC = "yes";
            ProtectHome = "yes";
            PrivateTmp = "yes";
            ProtectSystem = "strict";
            ProcSubset = "pid";

            SocketBindAllow = let
              port = lib.last (lib.splitString ":" cfg.bindAddr);
            in ["tcp:${port}"];
            SocketBindDeny = "any";

            LimitNOFILE = 4096;
            LimitNPROC = 128;
            MemoryMax = "512M";
            TasksMax = 256;

            ReadWritePaths = [
              cfg.mediaDir
              (dirOf cfg.databasePath)
            ];
          };
        };
      };
    };
  in {
    devShells.${system}.default = shell;
    nixosModules.blaz-service = service;
  };
}
