#!/usr/bin/env bash
# bench/run.sh [BUILD...]: the routine benchmark, a few minutes. Each BUILD is a directory holding a build of the module
# (vp9.js, vp9_bg.wasm), build/out when none is named, or the word libvpx for ffmpeg's libvpx-vp9 decoder. They decode
# the samples of bench/samples.sh alternately, round by round, on one thread and on four, and the medians over the
# rounds are printed per frame: the instructions and cycles of the whole process, which do not depend on the host's
# load, and the mean and median milliseconds.
#   ROUNDS=3 THREADS="1 4" FRAMES=all SAMPLES="name ..." CPUS1=2 CPUS4=2-5 QUIET=1.5
# On Linux the builds run on the same cores (taskset) and the counts are perf stat's of user space. On a Mac no
# process can be held to a core, so the rounds are what evens out a performance core against an efficiency one, and
# the counts are /usr/bin/time -l's, user and kernel together; a Mac's numbers are compared with a Mac's alone.
# The first module named is first checked against every sample's MD5s, and the host is waited on until it is quiet.
set -euo pipefail
cd "$(dirname "$0")/.."
[ $# -gt 0 ] || set -- build/out
rounds=${ROUNDS:-3}
os=$(uname -s)
# FRAMES=all, the default, is no limit: neither decoder is then given a count. Both are left unquoted where used.
limit='' fflimit=''
[ "${FRAMES:-all}" = all ] || { limit=$FRAMES fflimit="-frames:v $FRAMES"; }
# The default is five of the artifacts drive's bench folder, which has a quiet and a busy sample of every size captured.
samples=${SAMPLES:-quiet-1440x900-2col-lf busy-1280x800-1col-lf busy-1920x1080-2col-lf busy-2560x1600-4col-nolf busy-3840x2160-4col-nolf}
dir=$(bench/samples.sh $samples)

# The 1-, 5- and 15-minute loads. An idle Mac's background keeps its 1-minute load near 2, so its quiet is 3.
if [ "$os" = Darwin ]; then
  load() { sysctl -n vm.loadavg | tr -d '{}' | awk '{print $1, $2, $3}'; }
  quiet=${QUIET:-3}
else
  load() { cut -d' ' -f1-3 /proc/loadavg; }
  quiet=${QUIET:-1.5}
fi

# Waits, up to ten minutes, for the 1-minute load to fall under the quiet one, so no run is timed against a build or
# the last run.
quiet() {
  local waited=0
  while load | awk -v q="$quiet" '{exit !($1 >= q)}'; do
    [ $waited -lt 600 ] || { echo "!! load still $(load): measuring anyway"; return; }
    sleep 15; waited=$((waited + 15))
  done
}

# counted CPUS COMMAND...: runs the command, on those cores where it can be held to them, and prints its output and
# then the whole process's counts as lines '@instructions N' and '@cycles N'.
counted() {
  local cpus=$1 out; shift
  if [ "$os" = Darwin ]; then
    out=$(/usr/bin/time -l "$@" 2>&1)
    printf '%s\n' "$out"
    awk '/instructions retired/ {print "@instructions", $1} / cycles elapsed/ {print "@cycles", $1}' <<<"$out"
  else
    out=$(taskset -c "$cpus" perf stat -e instructions:u,cycles:u -x, "$@" 2>&1)
    printf '%s\n' "$out"
    awk -F, '$3 ~ /^instructions/ {print "@instructions", $1} $3 ~ /^cycles/ {print "@cycles", $1}' <<<"$out"
  fi
}

for s in $samples; do
  [ "$1" = libvpx ] && break
  VP9_WASM_DIR=$1 CHECK=1 bun bench/decode.ts "$dir/$s.ivf" 4 $limit | sed "s|^|$s: |"
done

log=$(mktemp); trap 'rm -f "$log"' EXIT
for t in ${THREADS:-1 4}; do
  [ "$t" = 1 ] && cpus=${CPUS1:-2} || cpus=${CPUS4:-2-5}
  [ "$os" = Darwin ] && where=unpinned || where="on cpus $cpus"
  for s in $samples; do
    quiet; : >"$log"
    for ((r = 0; r < rounds; r++)); do
      for b in "$@"; do
        if [ "$b" = libvpx ]; then
          out=$(counted "$cpus" ffmpeg -hide_banner -nostats -threads "$t" -c:v libvpx-vp9 -i "$dir/$s.ivf" $fflimit -benchmark -f null -)
          n=$(grep -o 'frame= *[0-9]*' <<<"$out" | tail -1 | tr -dc 0-9)
          # Its wall time over the frames; ffmpeg gives no time per frame, so the median is the mean.
          mean=$(grep -o 'rtime=[0-9.]*' <<<"$out" | cut -d= -f2 | awk -v n="$n" '{print $1 * 1000 / n}'); median=$mean
        else
          out=$(VP9_WASM_DIR=$b counted "$cpus" bun bench/decode.ts "$dir/$s.ivf" "$t" $limit)
          n=$(grep -o '^[0-9]* pictures' <<<"$out" | cut -d' ' -f1)
          mean=$(grep -o 'mean [0-9.]*' <<<"$out" | cut -d' ' -f2); median=$(grep -o 'median [0-9.]*' <<<"$out" | cut -d' ' -f2)
        fi
        i=$(grep '^@instructions' <<<"$out" | cut -d' ' -f2); c=$(grep '^@cycles' <<<"$out" | cut -d' ' -f2)
        awk -v b="$b" -v n="$n" -v i="$i" -v c="$c" -v a="$mean" -v m="$median" 'BEGIN {print b, i / n / 1e6, c / n / 1e6, a, m}' >>"$log"
      done
    done
    echo "-- $s, $n frames, $t thread(s) $where, medians of $rounds rounds, load $(load)"
    for b in "$@"; do
      med() { awk -v b="$b" '$1 == b' "$log" | cut -d' ' -f"$1" | sort -n | awk '{v[NR] = $1} END {print v[int((NR + 1) / 2)]}'; }
      printf '%-24s %8.1f M instr %8.1f M cycles   mean %6.2f ms  median %6.2f ms\n' "$b" "$(med 2)" "$(med 3)" "$(med 4)" "$(med 5)"
    done
  done
done
