{
  description = "Development environment for the personal video library (Rust + Svelte 5)";

  inputs = {
    nixpkgs.url = "nixpkgs";
  };

  outputs = { self, nixpkgs }:
    let
      supportedSystems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = f: nixpkgs.lib.genAttrs supportedSystems (system: f (import nixpkgs {
        inherit system;
        config = { allowUnfree = true; };
      }));
    in
    {
      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          name = "videoplayer-dev-shell";

          packages = with pkgs; [
            # Rust toolchain
            rustc
            cargo
            rustfmt
            clippy
            rust-analyzer

            # C libraries & build tools
            pkg-config
            openssl
            sqlite

            # Frontend toolchain
            nodejs_22

            # Media tools
            ffmpeg
            
            # Database CLI
            sqlx-cli
          ];

          # Environment variables
          RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
          PKG_CONFIG_PATH = "${pkgs.openssl.dev}/lib/pkgconfig:${pkgs.sqlite.dev}/lib/pkgconfig";
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath (with pkgs; [
            openssl
            sqlite
          ]);

          DATABASE_URL = "sqlite://data/videoplayer.db";

          shellHook = ''
            echo "=========================================================="
            echo "🎬 ZMydia development environment loaded through Nix Flake!"
            echo "   Rust:         $(rustc --version)"
            echo "   Cargo:        $(cargo --version)"
            echo "   Node:         $(node --version)"
            echo "   FFmpeg:       $(ffmpeg -version | head -n 1 | cut -d' ' -f1-3)"
            echo "   DATABASE_URL: $DATABASE_URL"
            echo "=========================================================="
          '';
        };
      });
    };
}
