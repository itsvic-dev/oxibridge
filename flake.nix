{
  description = "A very basic flake";

  inputs = { nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable"; };

  outputs = { self, nixpkgs }:
    let
      supportedSystems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
      nixpkgsFor = forAllSystems (system: nixpkgs.legacyPackages.${system});
    in {
      packages = forAllSystems (system:
        let pkgs = nixpkgsFor.${system};
        in { default = pkgs.callPackage ./nix/package.nix { }; });

      checks = forAllSystems (system:
        let pkgs = nixpkgsFor.${system};
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
