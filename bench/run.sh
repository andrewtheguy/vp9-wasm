#!/usr/bin/env bash
# bench/run.sh [BUILD...]: the routine benchmark, a few minutes. Each BUILD is a directory holding a build of the module
# (vp9.js, vp9_bg.wasm), build/out when none is named, or the word libvpx for ffmpeg's libvpx-vp9 decoder. Each module
# runs under each engine of ENGINES: Bun, whose JavaScriptCore is Safari's, and Node, whose V8 is Chrome's. The target
# is a Mac on ARM, and a Mac on x86 the next; each engine compiles the module otherwise on each, JavaScriptCore on x86
# making a shuffle of two vectors three instructions where V8 and JavaScriptCore on ARM make it one. They decode the
# samples of bench/samples.sh alternately, round by round, on one thread and on four, and the medians over the
# rounds are printed per frame after the first: the instructions and cycles of the whole process, which do not depend
# on the host's load, and the mean milliseconds, each less those of a run of the first frame alone, so that neither the
# start of the process nor the keyframe is in them; and the median milliseconds of the module's own clock.
#   ROUNDS=3 (5 on a Mac) THREADS="1 4" ENGINES="bun node" FRAMES=all SAMPLES="name ..." CPUS1=2 CPUS4=2-5 QUIET=1.5
# On Linux the builds run on the same cores (taskset) and the counts are perf stat's of user space. On a Mac no
# process can be held to a core, so the rounds, five there, are what evens out a performance core against an efficiency
# one and the load of a Mac in use, and the counts are /usr/bin/time -l's, user and kernel together; a Mac's numbers
# are compared with a Mac's alone. The first module named is first checked against every sample's MD5s under each
# engine, and a Linux host is waited on until it is quiet, a Mac only when QUIET names a load.
set -euo pipefail
cd "$(dirname "$0")/.."
[ $# -gt 0 ] || set -- build/out
os=$(uname -s)
[ "$os" = Darwin ] && rounds=${ROUNDS:-5} || rounds=${ROUNDS:-3}
engines=${ENGINES:-bun node}
# What is run: libvpx, and each module as ENGINE:BUILD.
runs=()
for b in "$@"; do
  if [ "$b" = libvpx ]; then runs+=(libvpx); else for e in $engines; do runs+=("$e:$b"); done; fi
done
# FRAMES=all, the default, is no limit: neither decoder is then given a count.
limit=''
[ "${FRAMES:-all}" = all ] || { [ "$FRAMES" -gt 1 ] || { echo "FRAMES must be more than the one subtracted" >&2; exit 2; }; limit=$FRAMES; }
# The default is a quiet desktop, a video full screen at four sizes, which is the busy case a change is judged on, and
# last a flood of text, the worst case, there for reference and no target. The artifacts drive's bench folder has a
# sample of every size, each named for wlshare's capture it is cut from.
samples=${SAMPLES:-terminal-1440x900-2col-lf video-1280x800-1col-lf video-1920x1080-2col-lf video-2560x1600-4col-nolf video-3840x2160-4col-nolf flood-1920x1080-2col-lf}
dir=$(bench/samples.sh $samples)

# The 1-, 5- and 15-minute loads. A Mac is seldom idle, and its background alone keeps its 1-minute load near 2, so it
# is not waited on unless QUIET names a load.
if [ "$os" = Darwin ]; then
  load() { sysctl -n vm.loadavg | tr -d '{}' | awk '{print $1, $2, $3}'; }
  quiet=${QUIET:-}
else
  load() { cut -d' ' -f1-3 /proc/loadavg; }
  quiet=${QUIET:-1.5}
fi

# Waits, up to ten minutes, for the 1-minute load to fall under the quiet one, so no run is timed against a build or
# the last run.
quiet() {
  local waited=0
  [ -n "$quiet" ] || return 0
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

# The first module named, under each engine.
first=''
for b in "$@"; do [ "$b" = libvpx ] || { first=$b; break; }; done
for e in $engines; do
  [ -n "$first" ] || break
  for s in $samples; do
    VP9_WASM_DIR=$first CHECK=1 "$e" bench/decode.ts "$dir/$s.ivf" 4 $limit | sed "s|^|$e: $s: |"
  done
done

# decoded RUN FRAMES: one run of libvpx or of ENGINE:BUILD on the sample, of at most FRAMES frames (all when empty), as the line
# 'FRAMES INSTRUCTIONS CYCLES MS MEDIAN': the frames decoded, the process's counts, the milliseconds its frames took
# together, and its median frame's.
decoded() {
  local b=$1 out n ms median
  if [ "$b" = libvpx ]; then
    out=$(counted "$cpus" ffmpeg -hide_banner -nostats -threads "$t" -c:v libvpx-vp9 -i "$dir/$s.ivf" ${2:+-frames:v $2} -benchmark -f null -)
    n=$(grep -o 'frame= *[0-9]*' <<<"$out" | tail -1 | tr -dc 0-9)
    # Its wall time over the frames; ffmpeg gives no time per frame, so its median is taken to be its mean.
    ms=$(grep -o 'rtime=[0-9.]*' <<<"$out" | cut -d= -f2 | awk '{print $1 * 1000}'); median=0
  else
    out=$(VP9_WASM_DIR=${b#*:} counted "$cpus" "${b%%:*}" bench/decode.ts "$dir/$s.ivf" "$t" $2)
    n=$(grep -o '^[0-9]* pictures' <<<"$out" | cut -d' ' -f1)
    ms=$(grep -o 'mean [0-9.]*' <<<"$out" | cut -d' ' -f2 | awk -v n="$n" '{print $1 * n}'); median=$(grep -o 'median [0-9.]*' <<<"$out" | cut -d' ' -f2)
  fi
  # A run that decoded nothing prints no count, given as 0 so that the fields after it stay in place.
  echo "${n:-0} $(grep '^@instructions' <<<"$out" | cut -d' ' -f2) $(grep '^@cycles' <<<"$out" | cut -d' ' -f2) $ms $median"
}

log=$(mktemp); trap 'rm -f "$log"' EXIT
for t in ${THREADS:-1 4}; do
  [ "$t" = 1 ] && cpus=${CPUS1:-2} || cpus=${CPUS4:-2-5}
  [ "$os" = Darwin ] && where=unpinned || where="on cpus $cpus"
  for s in $samples; do
    quiet; : >"$log"
    for ((r = 0; r < rounds; r++)); do
      for b in "${runs[@]}"; do
        read -r n i c ms median <<<"$(decoded "$b" "$limit")"
        [[ "$n" =~ ^[0-9]+$ && "$n" -gt 1 ]] || { echo "$b on $s: decoded '${n}' frames, not more than the one subtracted" >&2; exit 1; }
        read -r _ i1 c1 ms1 _ <<<"$(decoded "$b" 1)"
        awk -v b="$b" -v n="$n" -v i="$i" -v c="$c" -v ms="$ms" -v i1="$i1" -v c1="$c1" -v ms1="$ms1" -v m="$median" \
          'BEGIN {a = (ms - ms1) / (n - 1); print b, (i - i1) / (n - 1) / 1e6, (c - c1) / (n - 1) / 1e6, a, (b == "libvpx" ? a : m)}' >>"$log"
      done
    done
    echo "-- $s, $n frames, $t thread(s) $where, medians of $rounds rounds, load $(load)"
    for b in "${runs[@]}"; do
      med() { awk -v b="$b" '$1 == b' "$log" | cut -d' ' -f"$1" | sort -n | awk '{v[NR] = $1} END {print v[int((NR + 1) / 2)]}'; }
      printf '%-28s %8.1f M instr %8.1f M cycles   mean %6.2f ms  median %6.2f ms\n' "$b" "$(med 2)" "$(med 3)" "$(med 4)" "$(med 5)"
    done
  done
done
