{ lib, rustPlatform }:
rustPlatform.buildRustPackage rec {
  pname = "oxibridge";
  version = "0.1.0";
  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.difference ../. ../nix;
  };
  cargoLock.lockFile = ../Cargo.lock;
  buildAndTestSubdir = "oxibridge";

  meta = { mainProgram = "oxibridge"; };
}
