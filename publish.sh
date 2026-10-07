#!/usr/bin/env bash
# Build the release archive on the operator's own machine, and attach it to a
# release of this repository.
#
# Usage:
#   ./publish.sh
#
# Bump the version in rust/vp9-web/Cargo.toml, commit and push first. What gets
# built is `git archive HEAD`, not this directory, so nothing uncommitted or
# ignored can reach the archive. The tag is `v` and that version: the git tag
# of the commit the archive was built from, and the release holding the archive
# and its `SHA256SUMS`, whose notes give the digest remotex pins above what
# GitHub generates from the pull requests merged since the release before.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$here"

[ $# -eq 0 ] || { sed -n '2,/^set -euo pipefail$/p' "$0" | sed '$d; s/^# \{0,1\}//'; exit 2; }

# The module's crate is the one shipped, so its manifest names the release.
version="$(cargo metadata --manifest-path rust/vp9-web/Cargo.toml --no-deps --offline --format-version 1 \
  | jq -r '.packages[] | select(.name == "vp9-web") | .version')"
echo "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$' \
  || { echo "vp9-web's version is not X.Y.Z: '$version'" >&2; exit 1; }
tag="v$version"
archive="vp9-wasm-$tag.tar.gz"

# The tag names a commit, so the archive must be that commit's and the commit must
# be one anybody can fetch.
[ -z "$(git status --porcelain)" ] \
  || { echo "the working tree has uncommitted changes; a release is of a commit" >&2; exit 1; }
sha="$(git rev-parse HEAD)"
git fetch --quiet --tags origin
[ -n "$(git branch -r --contains "$sha")" ] \
  || { echo "$sha is on no branch of origin; push it first" >&2; exit 1; }
if git rev-parse -q --verify "refs/tags/$tag" >/dev/null; then
  echo "$tag already exists; bump the version" >&2
  exit 1
fi

# Before the build rather than after it: minutes of compiling is a poor way to
# learn that `gh` is logged in to the wrong account.
gh release list --limit 1 >/dev/null \
  || { echo "cannot read this repository's releases: gh auth login, with an account that can write to it" >&2; exit 1; }

# The commit, and only the commit, with this checkout's wasm-pack.
work="$here/tmp/publish-$tag"
rm -rf "$work"
mkdir -p "$work"
trap 'rm -rf "$work"' EXIT
git archive "$sha" | tar -x -C "$work"
ln -s "$here/node_modules" "$work/node_modules"
"$work/build.sh"

out="$work/dist"
[ -f "$out/$archive" ] || { echo "the build wrote no $archive" >&2; exit 1; }
# A corruption check, not a tamper check: it lives on the same release as the file
# it covers.
(cd "$out" && shasum -a 256 -- "$archive" >SHA256SUMS)
cat "$out/SHA256SUMS"

git tag "$tag" "$sha"
git push origin "refs/tags/$tag"

gh release create "$tag" --verify-tag --title "$tag" --generate-notes --notes "$(printf '%s\n\n    %s\n' \
  "\`$archive\` holds the module as remotex's build takes it: \`vp9.js\`, \`vp9_bg.wasm\` and \`vp9.d.ts\`. Its \`SHA256SUMS\` reads:" \
  "$(cat "$out/SHA256SUMS")")" \
  "$out/SHA256SUMS" "$out/$archive"
echo ">> published $tag; remotex pins it by $(cut -d' ' -f1 "$out/SHA256SUMS")"
