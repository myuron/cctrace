{ makeRustPlatform, rust-bin }:
let
  toolchain = rust-bin.stable.latest.default;
  rustPlatform = makeRustPlatform {
    cargo = toolchain;
    rustc = toolchain;
  };
in
rustPlatform.buildRustPackage {
  pname = "cctrace";
  version = "0.1.0";
  src = ../.;
  cargoLock.lockFile = ../Cargo.lock;
}
