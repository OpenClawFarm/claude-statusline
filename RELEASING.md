# Releasing

Releases ship one prebuilt binary for macOS on Apple Silicon; other platforms build from source.

Toolchain: `rustup` with the stable toolchain (Homebrew: `brew install rustup && rustup default stable`;
the formula is keg-only, so put `/opt/homebrew/opt/rustup/bin` on `PATH`).

1. Bump `version` in `Cargo.toml`, run `cargo test`, commit to `main`.
2. Build and package from a clean tree. Ownership is normalized so the archive carries no local user name:

   ```bash
   cargo build --release --locked
   A=claude-statusline-aarch64-apple-darwin.tar.gz
   COPYFILE_DISABLE=1 tar --uid 0 --gid 0 --uname root --gname wheel -czf /tmp/$A -C target/release claude-statusline
   (cd /tmp && shasum -a 256 $A > $A.sha256)
   ```

3. Tag the release commit `vX.Y.Z` and publish:

   ```bash
   gh release create vX.Y.Z --repo OpenClawFarm/claude-statusline /tmp/$A /tmp/$A.sha256
   ```

4. Install on each machine and verify against the published checksum:

   ```bash
   base=https://github.com/OpenClawFarm/claude-statusline/releases/latest/download
   A=claude-statusline-aarch64-apple-darwin.tar.gz
   cd /tmp && curl -fsSL -o sl.tgz $base/$A \
     && curl -fsSL $base/$A.sha256 | awk '{print $1"  sl.tgz"}' | shasum -a 256 -c - \
     && mkdir -p ~/.claude/bin && tar -xz -C ~/.claude/bin -f sl.tgz && rm sl.tgz
   ~/.claude/bin/claude-statusline --version
   echo '{"model":{"display_name":"X"},"context_window":{"remaining_percentage":70}}' | ~/.claude/bin/claude-statusline
   ```

   The installed binary's `shasum -a 256` should equal that of `target/release/claude-statusline`.
   Files fetched with `curl` carry no quarantine flag, so Gatekeeper doesn't block the unsigned binary.
