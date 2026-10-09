#!/usr/bin/env bash
# bench/samples.sh: the benchmark's sample streams, each an .ivf with libvpx's .ivf.framemd5, copied once from
# the bench folder of the artifacts drive (or VP9_BENCH_SOURCE) into tmp/bench, so that no run reads the drive. Prints
# the local directory. What the samples are is in that folder's README.
set -euo pipefail
cd "$(dirname "$0")/.."
local=tmp/bench source=${VP9_BENCH_SOURCE:-}
# The drive is /mnt/dasdata on Linux and /Volumes/dasdata on a Mac.
for d in /mnt/dasdata /Volumes/dasdata; do
  [ -n "$source" ] || { [ -d $d/backup/vp9-artifacts/bench ] && source=$d/backup/vp9-artifacts/bench; } || true
done
mkdir -p $local
if [ -n "$source" ] && [ -d "$source" ]; then
  for f in "$source"/*.ivf "$source"/*.ivf.framemd5; do
    name=$(basename "$f")
    if [ ! -e "$local/$name" ] || [ "$(wc -c < "$local/$name")" != "$(wc -c < "$f")" ]; then
      cp "$f" "$local/$name.part" && mv "$local/$name.part" "$local/$name"
    fi
  done
fi
ls $local/*.ivf >/dev/null 2>&1 || { echo "no samples in $local and no bench folder on the artifacts drive" >&2; exit 1; }
echo "$PWD/$local"
