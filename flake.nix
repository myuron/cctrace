{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    treefmt-nix.url = "github:numtide/treefmt-nix";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      nixpkgs,
      flake-utils,
      treefmt-nix,
      rust-overlay,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };
      in
      {
        formatter = treefmt-nix.lib.mkWrapper pkgs {
          projectRootFile = "flake.nix";
          programs = {
            nixfmt.enable = true;
            rustfmt.enable = true;
            prettier.enable = true;
          };
        };

        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            pkgs.rust-bin.stable.latest.default
            gh
            ripgrep
            fd
            jq
          ];
        };

        packages.default = pkgs.callPackage ./nix/rust.nix { };

        apps = {
          lint = {
            type = "app";
            program = toString (
              pkgs.writeShellScript "lint" ''
                cargo clippy
              ''
            );
          };

          test = {
            type = "app";
            program = toString (
              pkgs.writeShellScript "test" ''
                cargo test
              ''
            );
          };
        };
      }
    );
}
