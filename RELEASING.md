# Releasing gforce-node

This repo ships pre-built binaries via GitHub Releases. The customer-facing
installer (`install.sh` / `install.ps1`) downloads from
`https://github.com/nearminds/GforceNode/releases/latest/download/...`, so
"cutting a release" is the only way to get new code into customer hands.

## How to cut a release

1. Confirm `main` is green: open the [CI workflow][ci] and make sure the
   most recent run on `main` succeeded.
2. Open the [Cut release][cut] workflow on the Actions tab.
3. Click **Run workflow**, fill in:
   - **Tag to create** — e.g. `v0.1.0`, `v0.2.0-rc.1`. Must start with `v`
     and follow `vMAJOR.MINOR.PATCH` (or `-rc.N` / `-beta.N` / `-alpha.N`).
   - **Git ref** — leave as `main` unless you're cutting a hotfix from a
     branch.
   - **Allow red CI** — leave unchecked. Override only for emergency
     hotfixes where you know the failing check isn't relevant.
4. Click **Run workflow**.

The workflow validates the version, refuses if the tag already exists or
CI is red, then creates an annotated tag from the chosen ref. The tag
push immediately triggers the [Release workflow][rel] which builds for
six platforms in parallel, computes SHA256 checksums, and publishes the
release.

[ci]: https://github.com/nearminds/GforceNode/actions/workflows/ci.yml
[cut]: https://github.com/nearminds/GforceNode/actions/workflows/cut-release.yml
[rel]: https://github.com/nearminds/GforceNode/actions/workflows/release.yml

## What gets published

For each release tag the workflow uploads:

| Asset | Platform |
| --- | --- |
| `gforce-node-linux-x86_64.tar.gz` | Linux x86_64 |
| `gforce-node-linux-aarch64.tar.gz` | Linux ARM64 |
| `gforce-node-darwin-x86_64.tar.gz` | macOS Intel |
| `gforce-node-darwin-aarch64.tar.gz` | macOS Apple Silicon |
| `gforce-node-windows-x86_64.zip` | Windows x86_64 |
| `gforce-node-windows-aarch64.zip` | Windows ARM64 |
| `sha256sums.txt` | One line per asset, GNU coreutils format |

Each archive contains the `gforce-node` CLI and the `gforce-node-daemon`
service binary.

## Stable vs prerelease

- **Stable** tags (`v0.1.0`, `v1.2.3`) are published as normal GitHub
  releases. They become `releases/latest`, so any `curl|sh` installer
  that doesn't pin a version will pull them.
- **Prerelease** tags (`v0.2.0-rc.1`, `v1.0.0-beta.3`) are published with
  GitHub's `prerelease` flag set. They do **not** become `releases/latest`
  and are safe to publish for internal smoke-testing without breaking
  customer installs. Customers can still pin to them explicitly via
  `GFORCE_VERSION=v0.2.0-rc.1`.

The release workflow detects the suffix and applies the right flag
automatically — no separate flag needed at release-cut time.

## Post-release verification

The release workflow finishes with a smoke step that hits the public
`releases/latest/download/...` URLs the install script uses and confirms
each one returns a real archive (`HTTP 200` and >1 KB). This catches the
"asset upload silently dropped a matrix leg" failure mode where you'd
otherwise only find out when a customer installs.

If the smoke step fails, the workflow goes red **after** the release was
created. To recover:

1. Open the failing release on the Releases page.
2. Click "Edit", scroll to the bottom, **delete the release** (this
   removes the assets).
3. Delete the tag: `git push --delete origin vX.Y.Z`
4. Fix the underlying issue, then re-cut from the same version.

## Hotfix flow

For an urgent fix that can't wait for a green main:

1. Branch from the last good tag: `git checkout -b hotfix/v0.1.1 v0.1.0`
2. Cherry-pick or commit the fix.
3. Open a PR and merge to main if possible. If not (e.g. main is broken
   for unrelated reasons), keep the fix on the hotfix branch.
4. Cut release from the hotfix branch by setting the **Git ref** input
   to your hotfix branch name. Set **Allow red CI** to true only if the
   failing CI check on that ref is unrelated to the fix.

## Rollback

To roll back to a previous version, customers can pin via env var:

```sh
curl -sSL https://gforce.nearminds.org/install.sh | GFORCE_VERSION=v0.1.0 sh
```

We do not currently auto-promote an older release back to `latest`. If a
release needs to be unpublished, delete it from the Releases page — the
previous stable release will then become `latest` automatically.

## Local verification before cutting

You don't need to run the full release workflow to test a build:

```sh
cargo build --release --workspace
./target/release/gforce-node --version
./target/release/gforce-node-daemon --help
```

A passing local build doesn't replace the cross-platform CI matrix, but
it catches obvious breakage before you spend 10 minutes waiting for the
release workflow.
