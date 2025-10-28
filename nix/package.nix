{ lib, rustPlatform }:
rustPlatform.buildRustPackage rec {
  pname = "oxibridge";
  version = "0.1.0";
  src = lib.sources.sourceFilesBySuffices [ ".nix" ] (lib.cleanSource ../.);
  cargoLock = { lockFile = ../Cargo.lock; };

  meta = { mainProgram = "oxibridge"; };
}
