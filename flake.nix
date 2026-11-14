{
  description = "ZMydia personal video library";

  inputs.nixpkgs.url = "nixpkgs";

  outputs = { self, nixpkgs }:
    let
      lib = nixpkgs.lib;
      release = builtins.fromJSON (builtins.readFile ./nix/release.json);
      packageSystems = [ "x86_64-linux" ];
      devSystems = [ "x86_64-linux" "aarch64-linux" ];
      forPackageSystems = lib.genAttrs packageSystems;
      forDevSystems = lib.genAttrs devSystems;
      pkgsFor = system: import nixpkgs {
        inherit system;
        config.allowUnfree = true;
      };

      packageFor = system:
        let
          pkgs = pkgsFor system;
          platform = "linux-x86_64";
          archiveName = "zmydia-${release.version}-${platform}.tar.gz";
        in pkgs.stdenvNoCC.mkDerivation {
          pname = "zmydia";
          inherit (release) version;

          src = pkgs.fetchurl {
            url = "https://github.com/alissonlima086/ZMydia/releases/download/v${release.version}/${archiveName}";
            sha256 = release.linux_x86_64_sha256;
          };

          nativeBuildInputs = [ pkgs.makeWrapper ];
          sourceRoot = "zmydia-${release.version}-${platform}";
          dontBuild = true;

          installPhase = ''
            runHook preInstall
            mkdir -p $out
            cp -r bin lib README.md LICENSE THIRD-PARTY-NOTICES.md BUILD-INFO.json $out/
            runHook postInstall
          '';

          postFixup = ''
            wrapProgram $out/bin/zmydia-server \
              --prefix PATH : $out/lib/mymedia/bin \
              --set MYMEDIA_FRONTEND_DIR $out/lib/mymedia/frontend \
              --set MYMEDIA_EXTENSION_DIR $out/lib/mymedia/extension
          '';

          meta = {
            description = "Private, local-first personal video library";
            homepage = "https://github.com/alissonlima086/ZMydia";
            license = lib.licenses.mit;
            mainProgram = "zmydia-server";
            platforms = packageSystems;
          };
        };

      nixosModule = { config, lib, pkgs, ... }:
        let
          cfg = config.services.zmydia;
          commonEnvironment = [
            "PATH=${cfg.package}/lib/mymedia/bin:/run/current-system/sw/bin"
            "MYMEDIA_DATA_DIR=%h/.local/share/zmydia"
            "MYMEDIA_CACHE_DIR=%h/.cache/zmydia"
            "MYMEDIA_FRONTEND_DIR=${cfg.package}/lib/mymedia/frontend"
            "MYMEDIA_EXTENSION_DIR=${cfg.package}/lib/mymedia/extension"
            "MYMEDIA_PORT=${toString cfg.port}"
            "MYMEDIA_LOG_FORMAT=json"
          ];
        in {
          options.services.zmydia = {
            enable = lib.mkEnableOption "ZMydia personal video library";
            package = lib.mkOption {
              type = lib.types.package;
              default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
              description = "Prebuilt ZMydia release package to run.";
            };
            port = lib.mkOption {
              type = lib.types.port;
              default = 4173;
              description = "Loopback HTTP port used by ZMydia.";
            };
          };

          config = lib.mkIf cfg.enable {
            assertions = [{
              assertion = pkgs.stdenv.hostPlatform.system == "x86_64-linux";
              message = "The prebuilt ZMydia release currently supports x86_64-linux only.";
            }];
            environment.systemPackages = [ cfg.package ];

            systemd.user.services.zmydia = {
              description = "ZMydia personal video library";
              after = [ "network-online.target" ];
              wants = [ "network-online.target" ];
              wantedBy = [ "default.target" ];
              serviceConfig = {
                ExecStart = "${cfg.package}/bin/zmydia-server serve";
                Restart = "on-failure";
                RestartSec = 5;
                TimeoutStopSec = 45;
                Environment = commonEnvironment;
                EnvironmentFile = "-%h/.config/zmydia/zmydia.env";
                NoNewPrivileges = true;
                PrivateTmp = true;
                ProtectSystem = "full";
                ProtectKernelTunables = true;
                ProtectKernelModules = true;
                ProtectControlGroups = true;
                RestrictRealtime = true;
                RestrictSUIDSGID = true;
                LockPersonality = true;
                MemoryHigh = "1500M";
                MemoryMax = "2500M";
                CPUQuota = "200%";
              };
            };

            systemd.user.services.zmydia-backup = {
              description = "Create a ZMydia library backup";
              after = [ "zmydia.service" ];
              serviceConfig = {
                Type = "oneshot";
                ExecStart = "${cfg.package}/bin/zmydia-server backup";
                Environment = commonEnvironment;
                EnvironmentFile = "-%h/.config/zmydia/zmydia.env";
                NoNewPrivileges = true;
                PrivateTmp = true;
                ProtectSystem = "full";
              };
            };

            systemd.user.timers.zmydia-backup = {
              description = "Create a daily ZMydia library backup";
              wantedBy = [ "timers.target" ];
              timerConfig = {
                OnCalendar = "daily";
                Persistent = true;
                RandomizedDelaySec = "15m";
              };
            };
          };
        };
    in {
      packages = forPackageSystems (system: {
        default = packageFor system;
        zmydia = packageFor system;
      });

      apps = forPackageSystems (system: {
        default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/zmydia-server";
          meta.description = "Run the prebuilt ZMydia server";
        };
      });

      checks = forPackageSystems (system:
        let
          pkgs = pkgsFor system;
          evaluated = lib.nixosSystem {
            inherit system;
            modules = [
              nixosModule
              {
                services.zmydia.enable = true;
                system.stateVersion = "26.05";
              }
            ];
          };
        in {
          nixos-module = pkgs.writeText "zmydia-module-check"
            evaluated.config.systemd.user.services.zmydia.serviceConfig.ExecStart;
        });

      nixosModules.default = nixosModule;
      nixosModules.zmydia = nixosModule;

      devShells = forDevSystems (system:
        let pkgs = pkgsFor system;
        in {
          default = pkgs.mkShell {
            name = "zmydia-dev-shell";
            packages = with pkgs; [
              rustc
              cargo
              rustfmt
              clippy
              rust-analyzer
              pkg-config
              openssl
              sqlite
              nodejs_22
              ffmpeg
              yt-dlp
              curl
              sqlx-cli
            ];
            RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
            PKG_CONFIG_PATH = "${pkgs.openssl.dev}/lib/pkgconfig:${pkgs.sqlite.dev}/lib/pkgconfig";
            LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [ pkgs.openssl pkgs.sqlite ];
            DATABASE_URL = "sqlite://data/videoplayer.db";
          };
        });
    };
}
