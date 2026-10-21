# Agent instructions

Rust + GPUI rebuild of nanoMIDIPlayer (Windows). Read `README.md` for the architecture.

## Building
- Use `cargo build --release` for normal work. It is incremental and fast after the first build.
- Only use `cargo build --profile dist` when the user wants a final, distributable binary.
- `rust-lld` is configured as the linker in `.cargo/config.toml`. Don't remove it.

## Always clean up old builds (save disk space)
GPUI builds are large (several GB per profile). After every successful build:

1. Delete the output folders of the profiles you did **not** just build:
   - built `release` → `rm -rf target/debug target/dist`
   - built `dist` → `rm -rf target/debug` (keep `target/release`; it holds the cache that makes rebuilds fast)
   - built `debug` → `rm -rf target/dist`
2. Delete stale incremental caches that are older than the current one:
   `find target/*/incremental -mindepth 1 -maxdepth 1 -type d -mtime +1 -exec rm -rf {} +`
3. Never run `cargo clean` on the whole `target/` folder unless the user asks. Wiping `target/release` forces the ~10-minute cold rebuild.
