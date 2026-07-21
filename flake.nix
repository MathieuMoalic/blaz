{
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

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
        platformVersions = ["36" "35" "34"];
        buildToolsVersions = ["36.0.0" "35.0.0" "34.0.0"];
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
        tmux
        git
        bashInteractive
        coreutils
        gnugrep
        gnused
        gawk
        findutils
        which
        curl
        jq
        procps
        lsof
        ripgrep
        fd
        netcat-openbsd
        chromium
        chromedriver
      ];

      RUSTFLAGS = "-C link-arg=-fuse-ld=mold";

      ANDROID_SDK_ROOT = sdkRoot;
      ANDROID_HOME = sdkRoot;
      JAVA_HOME = "${pkgs.jdk17}/lib/openjdk";
      CHROME_EXECUTABLE = "${pkgs.chromium}/bin/chromium";
    };

    webBuild = pkgs.flutter.buildFlutterApplication {
      pname = "blaz-web";
      version = "2.8.7";
      src = pkgs.lib.cleanSource ./flutter;
      autoPubspecLock = ./flutter/pubspec.lock;
      targetFlutterPlatform = "web";
    };

    package = pkgs.rustPlatform.buildRustPackage {
      pname = "blaz";
      version = "2.8.7";
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

    prebuilt = pkgs.stdenvNoCC.mkDerivation {
      pname = "blaz";
      version = "2.8.7";

      src = pkgs.fetchurl {
        url = "https://github.com/MathieuMoalic/blaz/releases/download/v2.8.7/blaz-v2.8.7-x86_64-linux.tar.gz";
        hash = "sha256-3BRlO+uZmmvvRPqB+zLiBpmE6uL0JsvBFcdHll6iUo0=";
      };

      sourceRoot = ".";

      nativeBuildInputs = [pkgs.patchelf];

      installPhase = ''
        install -Dm755 blaz-v2.8.7-x86_64-linux $out/bin/blaz
        patchelf \
          --set-interpreter ${pkgs.stdenv.cc.bintools.dynamicLinker} \
          --set-rpath ${lib.makeLibraryPath [pkgs.stdenv.cc.cc.lib pkgs.glibc]} \
          $out/bin/blaz
      '';

      meta = with lib; {
        description = "Recipe manager backend (prebuilt)";
        homepage = "https://github.com/MathieuMoalic/blaz";
        license = licenses.gpl3;
        platforms = ["x86_64-linux"];
        maintainers = [];
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
          description = "The blaz package to use.";
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

        passwordHash = lib.mkOption {
          type = lib.types.nullOr lib.types.str;
          default = null;
          description = "Argon2 password hash. Generate with: blaz hash-password";
        };

        passwordHashFile = lib.mkOption {
          type = lib.types.nullOr lib.types.path;
          default = null;
          description = "Path to file containing password hash (for sops-nix)";
        };

        jwtSecret = lib.mkOption {
          type = lib.types.nullOr lib.types.str;
          default = null;
          description = "JWT secret. If not set, generates a random one.";
        };

        jwtSecretFile = lib.mkOption {
          type = lib.types.nullOr lib.types.path;
          default = null;
          description = "Path to file containing JWT secret (for sops-nix)";
        };

        llmApiUrl = lib.mkOption {
          type = lib.types.str;
          default = "https://openrouter.ai/api/v1";
          description = "LLM API endpoint URL";
        };

        llmApiKey = lib.mkOption {
          type = lib.types.nullOr lib.types.str;
          default = null;
          description = "LLM API key";
        };

        llmApiKeyFile = lib.mkOption {
          type = lib.types.nullOr lib.types.path;
          default = null;
          description = "Path to file containing LLM API key (for sops-nix)";
        };

        llmModel = lib.mkOption {
          type = lib.types.str;
          default = "deepseek/deepseek-v4-flash";
          description = "LLM model name to use";
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

        systemd.tmpfiles.rules = [
          "d ${dirOf cfg.databasePath} 0750 blaz blaz - -"
          "d ${cfg.mediaDir} 0750 blaz blaz - -"
          "d ${dirOf cfg.logFile} 0750 blaz blaz - -"
          "f ${cfg.logFile} 0640 blaz blaz - -"
        ];

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
              BLAZ_LLM_API_URL = cfg.llmApiUrl;
              BLAZ_LLM_MODEL = cfg.llmModel;
            }
            // lib.optionalAttrs (cfg.corsOrigin != null) {BLAZ_CORS_ORIGIN = cfg.corsOrigin;}
            // lib.optionalAttrs (cfg.passwordHash != null) {BLAZ_PASSWORD_HASH = cfg.passwordHash;}
            // lib.optionalAttrs (cfg.jwtSecret != null) {BLAZ_JWT_SECRET = cfg.jwtSecret;}
            // lib.optionalAttrs (cfg.llmApiKey != null) {BLAZ_LLM_API_KEY = cfg.llmApiKey;};

          script = let
            passwordHashLoader =
              if cfg.passwordHashFile != null
              then ''export BLAZ_PASSWORD_HASH="$(cat ${cfg.passwordHashFile})"''
              else "";
            jwtSecretLoader =
              if cfg.jwtSecretFile != null
              then ''export BLAZ_JWT_SECRET="$(cat ${cfg.jwtSecretFile})"''
              else "";
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
            Restart = "always";
            RestartSec = "5s";
            NoNewPrivileges = "yes";
            PrivateTmp = "yes";
            ProtectSystem = "strict";
            ReadWritePaths = [
              (dirOf cfg.databasePath)
              cfg.mediaDir
            ];
            SocketBindAllow = let
              port = lib.last (lib.splitString ":" cfg.bindAddr);
            in ["tcp:${port}"];
            SocketBindDeny = "any";
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
      prebuilt = prebuilt;
      web = webBuild;
    };
  };
}
