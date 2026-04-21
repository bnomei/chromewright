# Release

This repository can publish `chromewright` to crates.io and attach prebuilt binary archives to GitHub Releases.

## Reserve The Crate Name

1. Ensure `Cargo.toml` metadata is correct.
2. Log in locally:

```bash
cargo login
```

3. Run a final local packaging check:

```bash
cargo package --locked
```

4. Publish the crate to crates.io:

```bash
cargo publish
```

Publishing once is enough to reserve the `chromewright` crate name.

## Tag A Binary Release

1. Update `version` in `Cargo.toml`.
2. Commit the release changes.
3. Create and push a version tag:

```bash
git tag v0.2.3
git push origin v0.2.3
```

4. GitHub Actions will build release archives and attach them to the GitHub Release for that tag.

## Local Smoke Checks

```bash
cargo fmt --all -- --check
cargo test --all-targets --all-features
cargo package --locked
cargo install --path . --root /tmp/chromewright-install-smoke --force
```
