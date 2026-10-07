#!/usr/bin/env bash
# Build the VP9 WebAssembly decoder for remotex's page: rust/vp9-web, the `vp9`
# crate behind the few calls the page's decode worker makes, with threads and
# SIMD, through wasm-pack. Writes build/out/vp9.js (wasm-bindgen's ES module
# glue) and build/out/vp9.wasm, and the release archive
# dist/vp9-wasm-vX.Y.Z.tar.gz holding the two, X.Y.Z being vp9-web's version.
#
# The nightly rust/vp9-web/rust-toolchain.toml names is installed by rustup on
# the first build; wasm-pack comes from `bun install`, run here where it is
# missing.
#
#   ./build.sh
set -euo pipefail
cd "$(dirname "$0")"

version=$(cargo metadata --manifest-path rust/vp9-web/Cargo.toml --no-deps --offline --format-version 1 \
  | jq -r '.packages[] | select(.name == "vp9-web") | .version')
[ -x node_modules/.bin/wasm-pack ] || bun install --frozen-lockfile

rm -rf build/pkg build/out dist
node_modules/.bin/wasm-pack build rust/vp9-web --release --target web --no-pack \
  --out-name vp9 --out-dir "$PWD/build/pkg"
mkdir -p build/out dist
cp build/pkg/vp9.js build/out/vp9.js
cp build/pkg/vp9_bg.wasm build/out/vp9.wasm

# The release: the two files the page loads, and nothing else. GNU tar with fixed
# names, owners and times, so one build's archive is byte for byte the next's
# from the same toolchain.
tar --sort=name --owner=0 --group=0 --numeric-owner --mtime=@0 \
  -C build/out -cf - vp9.js vp9.wasm | gzip -9n >"dist/vp9-wasm-v$version.tar.gz"
ls -l build/out dist
