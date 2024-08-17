{
  description = "Clovis flake";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};

        clovis = pkgs.rustPlatform.buildRustPackage {
          pname = "clovis";
          version = "0.1.0";
          src = ./.;
          cargoLock = { lockFile = ./Cargo.lock; };
        };
      in { packages.default = clovis; }) // {
        nixosModules.default = { config, lib, pkgs, ... }:
          let cfg = config.programs.clovis;
          in {
            options.programs.clovis = {
              enable = lib.mkEnableOption "Clovis program";
            };

            config = lib.mkIf cfg.enable {
              environment.systemPackages =
                [ self.packages.${pkgs.system}.default ];
            };
          };
      };
}
