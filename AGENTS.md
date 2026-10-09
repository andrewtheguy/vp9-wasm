# Repository instructions

Keep this file to rules that change how work is performed. Design explanations
and build details belong in the [README](README.md).

## Workflow

- Strict no backward-compatibility or legacy paths.
- No squash merges
- No change logs
- The decoder takes 4:4:4 at 8 bits and nothing else. Refuse another shape of
  stream by name; do not add 4:2:0, more bits, or compound prediction.
- After a decoder change, run the native check and `bun test`, once each: the
  first is where the scalar paths run, the second the module's SIMD ones.
- After test changes, run `bun test` and `bun run typecheck`, once each.
- Run `bun run fixtures` only when a spec in `test/fixtures.ts` changes, and
  commit what it writes to `test/data`.
- To measure a change, run `bench/run.sh BASE build/out`, minutes on short
  samples. Run whole captures only when asked, and never read a stream from
  the artifacts drive: copy it under `tmp/` first.
- Do not run `cargo fmt`.
- Put temporary files and test configuration under `tmp/`. Always run local
  Python through `uv` (GitHub Actions excluded).
