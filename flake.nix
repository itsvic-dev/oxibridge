{
  description = "A very basic flake";

  inputs = { nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable"; };

  outputs = { self, nixpkgs }:
    let
      supportedSystems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
    in {
      packages = forAllSystems (system:
        let pkgs = nixpkgs.legacyPackages.${system};
        in { default = pkgs.callPackage ./nix/package.nix { }; });

      checks = forAllSystems (system:
        let pkgs = nixpkgs.legacyPackages.${system};
        in { default = pkgs.callPackage ./nix/test.nix { inherit self; }; });

      nixosModules = rec {
        oxibridge = import ./nix/module.nix self;
        default = oxibridge;
      };

      hydraJobs = {
        packages = { inherit (self.packages) x86_64-linux aarch64-linux; };
        checks = { inherit (self.checks) x86_64-linux aarch64-linux; };
      };
    };
}
