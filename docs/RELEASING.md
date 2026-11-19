# Releasing

1. Pick the version (SemVer; pre-releases like `0.2.0-rc1` are marked as prerelease on GitHub automatically).
2. Bump it **in all three places**: `package.json`, `src-tauri/Cargo.toml` (then `cargo update -p coreforge --offline` or any build to refresh `Cargo.lock`),
   and `src-tauri/tauri.conf.json`. Check with `node scripts/check-versions.mjs` (CI runs it too).
3. Update `CHANGELOG.md` (move `Unreleased` items under the new version and date) and the `<release>` entry in
   `src-tauri/packaging/com.coreforge.app.metainfo.xml`.
4. Commit, then tag and push: `git tag v0.2.0 && git push origin main v0.2.0`.
5. The `Release` workflow (`.github/workflows/release.yml`) runs on `v*` tags: it fails if the tag differs from the app version, builds the
   `.deb`, `.rpm` and AppImage on ubuntu-22.04 (oldest glibc we support), writes `SHA256SUMS`, and creates the GitHub release with generated notes.
6. Download the artifacts and test the `.deb` (below) before announcing.

## Testing the .deb in a clean container

```sh
docker run --rm -it -v "$PWD/src-tauri/target/release/bundle/deb:/pkg:ro" ubuntu:24.04 bash
apt-get update && apt-get install -y /pkg/coreforge_*_amd64.deb
dpkg -L coreforge   # helper only in /usr/libexec/coreforge/, policy in /usr/share/polkit-1/actions/
ls -l /usr/libexec/coreforge/coreforge-helper   # root:root, 755
apt-get remove -y coreforge
```

The GUI needs a display, so a container only checks installation and file ownership; run the app itself in a VM.
