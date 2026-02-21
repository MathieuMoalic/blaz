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
        sqlx-cli
        watchexec
      ];

      RUSTFLAGS = "-C link-arg=-fuse-ld=mold";

      ANDROID_SDK_ROOT = sdkRoot;
      ANDROID_HOME = sdkRoot;
      JAVA_HOME = "${pkgs.jdk17}/lib/openjdk";
    };

    webBuild = pkgs.flutter.buildFlutterApplication {
      pname = "blaz-web";
      version = "0.1.0";
      src = pkgs.lib.cleanSource ./flutter;
      autoPubspecLock = ./flutter/pubspec.lock;
      targetFlutterPlatform = "web";
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

      # Copy web build before building rust
      preBuild = ''
        mkdir -p web_build
        cp -r ${webBuild}/* web_build/
      '';

      doCheck = false;

      meta = with lib; {
        description = "Recipe manager backend";
        homepage = "https://github.com/MathieuMoalic/blaz";
        license = licenses.gpl3;
        maintainers = [];
      };
    };

    # Prebuilt binary from GitHub releases (faster, no compilation needed)
    # NOTE: This always points to a previous stable release since we can't
    # know the hash until after the release is built. This is intentional
    # and keeps deployment fast while source builds always give you latest.
    prebuiltPackage = pkgs.stdenvNoCC.mkDerivation rec {
      pname = "blaz";
      version = "1.1.0";

      src = pkgs.fetchurl {
        url = "https://github.com/MathieuMoalic/blaz/releases/download/v${version}/blaz-v${version}-x86_64-linux";
        sha256 = "sha256-6RMkpmmlZm5YuIRTrW75zllTQckCUiFQ9NfvQNKsxt0=";
      };

      dontUnpack = true;

      installPhase = ''
        runHook preInstall
        mkdir -p "$out/bin"
        cp "$src" "$out/bin/blaz"
        chmod +x "$out/bin/blaz"
        runHook postInstall
      '';

      meta = with lib; {
        description = "Recipe manager backend (prebuilt binary from latest release)";
        homepage = "https://github.com/MathieuMoalic/blaz";
        license = licenses.gpl3;
        platforms = ["x86_64-linux"];
        mainProgram = "blaz";
      };
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
        enable = lib.mkEnableOption "Blaz recipe manager backend";

        package = lib.mkOption {
          type = lib.types.package;
          default = package;
          defaultText = lib.literalExpression "package (built from source)";
          description = "The blaz package to use. Set to prebuiltPackage to use prebuilt binaries from GitHub releases.";
        };

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
          default = "/var/lib/blaz/blaz.log";
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

        # Authentication options
        passwordHash = lib.mkOption {
          type = lib.types.nullOr lib.types.str;
          default = null;
          description = "Argon2 password hash for authentication. Generate with: blaz hash-password";
        };

        passwordHashFile = lib.mkOption {
          type = lib.types.nullOr lib.types.path;
          default = null;
          description = "Path to file containing password hash (for use with sops-nix)";
        };

        jwtSecret = lib.mkOption {
          type = lib.types.nullOr lib.types.str;
          default = null;
          description = "JWT secret for token signing. If not set, generates a random one (tokens won't persist across restarts)";
        };

        jwtSecretFile = lib.mkOption {
          type = lib.types.nullOr lib.types.path;
          default = null;
          description = "Path to file containing JWT secret (for use with sops-nix)";
        };

        # LLM options
        llmApiKey = lib.mkOption {
          type = lib.types.nullOr lib.types.str;
          default = null;
          description = "LLM API key for recipe parsing and macro estimation";
        };

        llmApiKeyFile = lib.mkOption {
          type = lib.types.nullOr lib.types.path;
          default = null;
          description = "Path to file containing LLM API key (for use with sops-nix)";
        };

        llmModel = lib.mkOption {
          type = lib.types.str;
          default = "deepseek/deepseek-chat";
          description = "LLM model to use";
        };

        llmApiUrl = lib.mkOption {
          type = lib.types.str;
          default = "https://openrouter.ai/api/v1";
          description = "LLM API URL";
        };

        systemPromptImport = lib.mkOption {
          type = lib.types.nullOr lib.types.str;
          default = null;
          description = "Custom system prompt for recipe import (uses default if not set)";
        };

        systemPromptMacros = lib.mkOption {
          type = lib.types.nullOr lib.types.str;
          default = null;
          description = "Custom system prompt for macro estimation (uses default if not set)";
        };
      };

      config = lib.mkIf cfg.enable {
        assertions = [
          {
            assertion = cfg.passwordHash != null || cfg.passwordHashFile != null;
            message = "services.blaz.passwordHash or services.blaz.passwordHashFile must be set";
          }
          {
            assertion = !(cfg.passwordHash != null && cfg.passwordHashFile != null);
            message = "services.blaz.passwordHash and services.blaz.passwordHashFile are mutually exclusive";
          }
          {
            assertion = !(cfg.jwtSecret != null && cfg.jwtSecretFile != null);
            message = "services.blaz.jwtSecret and services.blaz.jwtSecretFile are mutually exclusive";
          }
          {
            assertion = !(cfg.llmApiKey != null && cfg.llmApiKeyFile != null);
            message = "services.blaz.llmApiKey and services.blaz.llmApiKeyFile are mutually exclusive";
          }
        ];

        users.users.blaz = {
          isSystemUser = true;
          group = "blaz";
          home = "/var/lib/blaz";
          createHome = true;
        };
        users.groups.blaz = {};

        # Create directories before service starts (required due to ProtectSystem = strict)
        systemd.tmpfiles.rules = [
          # Database directory
          "d ${dirOf cfg.databasePath} 0750 blaz blaz - -"

          # Media directory
          "d ${cfg.mediaDir} 0750 blaz blaz - -"

          # Log directory and file
          "d ${dirOf cfg.logFile} 0750 blaz blaz - -"
          "f ${cfg.logFile} 0640 blaz blaz - -"
        ];

        systemd.services.blaz = {
          description = "Blaz recipe manager backend";
          after = ["network.target"];
          wantedBy = ["multi-user.target"];

          # Basic environment variables (non-secrets)
          environment =
            {
              BLAZ_BIND_ADDR = cfg.bindAddr;
              BLAZ_DATABASE_PATH = cfg.databasePath;
              BLAZ_MEDIA_DIR = cfg.mediaDir;
              BLAZ_LOG_FILE = cfg.logFile;
              BLAZ_LLM_MODEL = cfg.llmModel;
              BLAZ_LLM_API_URL = cfg.llmApiUrl;
            }
            // lib.optionalAttrs (cfg.corsOrigin != null) {
              BLAZ_CORS_ORIGIN = cfg.corsOrigin;
            }
            // lib.optionalAttrs (cfg.passwordHash != null) {
              BLAZ_PASSWORD_HASH = cfg.passwordHash;
            }
            // lib.optionalAttrs (cfg.jwtSecret != null) {
              BLAZ_JWT_SECRET = cfg.jwtSecret;
            }
            // lib.optionalAttrs (cfg.llmApiKey != null) {
              BLAZ_LLM_API_KEY = cfg.llmApiKey;
            }
            // lib.optionalAttrs (cfg.systemPromptImport != null) {
              BLAZ_SYSTEM_PROMPT_IMPORT = cfg.systemPromptImport;
            }
            // lib.optionalAttrs (cfg.systemPromptMacros != null) {
              BLAZ_SYSTEM_PROMPT_MACROS = cfg.systemPromptMacros;
            };

          # Script to load secrets from files before starting
          script = let
            # Load password hash from file if specified
            passwordHashLoader =
              if cfg.passwordHashFile != null
              then ''export BLAZ_PASSWORD_HASH="$(cat ${cfg.passwordHashFile})"''
              else "";

            # Load JWT secret from file if specified
            jwtSecretLoader =
              if cfg.jwtSecretFile != null
              then ''export BLAZ_JWT_SECRET="$(cat ${cfg.jwtSecretFile})"''
              else "";

            # Load LLM API key from file if specified
            llmApiKeyLoader =
              if cfg.llmApiKeyFile != null
              then ''export BLAZ_LLM_API_KEY="$(cat ${cfg.llmApiKeyFile})"''
              else "";
          in ''
            ${passwordHashLoader}
            ${jwtSecretLoader}
            ${llmApiKeyLoader}

            exec ${cfg.package}/bin/blaz \
              ${lib.concatStringsSep " " (
              lib.optionals (cfg.verbosity > 0) (lib.genList (_: "-v") cfg.verbosity)
              ++ lib.optionals (cfg.verbosity < 0) (lib.genList (_: "-q") (- cfg.verbosity))
            )}
          '';

          serviceConfig = {
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
    packages.${system} = {
      default = package;
      backend = package;
      web = webBuild;
      prebuilt = prebuiltPackage;
    };
  };
}
