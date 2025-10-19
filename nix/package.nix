{ rustPlatform }:
rustPlatform.buildRustPackage rec {
  pname = "oxibridge";
  version = "0.1.0";
  src = ../.;
  cargoLock = { lockFile = ../Cargo.lock; };

  meta = { mainProgram = "oxibridge"; };
}
