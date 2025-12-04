# GitHub Actions CI/CD Design

**Date:** 2025-12-03
**Project:** matrix-bridge
**Status:** Validated

## Overview

This document describes the GitHub Actions CI/CD system for the matrix-bridge project. The system automatically checks code quality on every commit and automatically builds releases for Linux and macOS when tags are pushed.

## Architecture

Two main workflows form the system:

1. **CI Workflow** (`ci.yml`) - Quality gates for every push and PR
2. **Release Workflow** (`release.yml`) - Build and publish releases on git tags

Both workflows use aggressive caching strategies to maximize build speed while maintaining cache invalidation when dependencies change.

## CI Workflow (`ci.yml`)

### Triggers

- Push to any branch
- All pull requests

### Job Structure

Three parallel jobs provide fast feedback:

**Job 1: Format Check**
- Runs `cargo fmt --all -- --check`
- Rejects improperly formatted code
- Completes in ~10 seconds
- Requires no caching

**Job 2: Clippy Lint**
- Runs `cargo clippy --all-targets --all-features -- -D warnings`
- Treats all warnings as errors (strict mode)
- Uses aggressive caching strategy
- Takes ~30 seconds with cache, ~2 minutes cold

**Job 3: Test Suite**
- Runs `cargo test --all-features`
- Executes all 8 unit tests
- Uses aggressive caching strategy
- Takes ~30 seconds with cache, ~3 minutes cold

### Rust Toolchain

- Uses stable channel (always latest)
- Automatically updates to newest stable Rust
- Requires no version pinning

### Caching Strategy

Aggressive caching with smart invalidation:

**Cache Key:** `rust-cache-${{ runner.os }}-${{ hashFiles('**/Cargo.lock') }}`

**Cached Paths:**
- `~/.cargo/bin/`
- `~/.cargo/registry/index/`
- `~/.cargo/registry/cache/`
- `~/.cargo/git/db/`
- `target/`

**Invalidation:** The cache automatically invalidates when `Cargo.lock` changes.

**Restore Fallbacks:** OS-specific fallbacks restore previous builds when the lockfile changes.

### Branch Protection

Configure GitHub to require all three jobs to pass before merging PRs.

## Release Workflow (`release.yml`)

### Trigger

Pushing a git tag matching `v*` pattern:

```bash
git tag v0.2.0
git push origin v0.2.0
```

### Build Matrix

Cross-platform builds using matrix strategy:

| Platform | Target Triple | Artifact Name |
|----------|--------------|---------------|
| Linux x86_64 | `x86_64-unknown-linux-gnu` | `matrix-bridge-linux-x86_64` |
| macOS x86_64 | `x86_64-apple-darwin` | `matrix-bridge-macos-x86_64` |

### Build Steps

For each platform, the workflow:

1. Checks out code with full git history (for changelog generation)
2. Installs Rust stable toolchain with target support
3. Restores aggressive cache (same strategy as CI)
4. Builds release binary: `cargo build --release --target $TARGET`
5. Strips debug symbols: `strip target/$TARGET/release/matrix-bridge`
6. Renames to artifact name (e.g., `matrix-bridge-linux-x86_64`)
7. Uploads artifact for release creation

### Release Creation

After both platform builds complete:

1. Downloads both platform artifacts
2. Generates changelog from commits between current tag and previous tag
3. Parses conventional commits (feat/fix/docs/chore)
4. Creates GitHub release with:
   - Auto-generated release notes grouped by type
   - Both platform binaries attached
   - Tag name as release title

### Release Notes Format

Auto-generated from conventional commits:

```markdown
## Features
- feat: description of feature

## Bug Fixes
- fix: description of fix

## Documentation
- docs: description of doc change

## Chores
- chore: description of chore
```

Users may edit release notes in the GitHub UI after creation.

## Binary Naming Convention

A simple platform suffix format:
- `matrix-bridge-linux-x86_64`
- `matrix-bridge-macos-x86_64`

This naming clearly indicates which binary to download for each platform.

## Performance Expectations

### CI Workflow
- **With cache hit:** ~30 seconds total
- **Cold build:** ~3 minutes total
- **Format check:** ~10 seconds

### Release Workflow
- **Full build (both platforms):** ~5-7 minutes
- **Per-platform build:** ~3-4 minutes

## Future Enhancements

The following additions remain candidates for future implementation:

- ARM64 builds (Linux and macOS)
- Security audit with cargo-audit
- License compliance checking with cargo-deny
- Nightly Rust builds for early warning
- Performance benchmarking
- Docker image builds

## Implementation Files

The implementation creates:

- `.github/workflows/ci.yml` - CI workflow
- `.github/workflows/release.yml` - Release workflow
- `.github/dependabot.yml` - Optional: automated dependency updates

## Testing the Workflows

### Testing CI
```bash
git checkout -b test-ci
# Make a change
git commit -m "test: verify CI workflow"
git push origin test-ci
# Open PR and verify all three jobs pass
```

### Testing Release
```bash
git tag v0.2.0-test
git push origin v0.2.0-test
# Verify workflow runs and creates release with binaries
# Delete test release and tag after verification
```

## Dependencies

Workflows require only standard GitHub Actions and Rust tooling. No new dependencies are needed in `Cargo.toml`.

## Maintenance

- **Rust updates:** Automatic via stable channel
- **Action updates:** Dependabot can manage GitHub Action versions
- **Cache cleanup:** GitHub automatically evicts old caches after 7 days

## Success Criteria

The CI/CD system succeeds when:

1. All pushes and PRs trigger quality checks automatically
2. Failed checks prevent merging (with branch protection)
3. Tagging a version creates a release with binaries within 10 minutes
4. Binaries work on target platforms (Linux and macOS x86_64)
5. Release notes accurately reflect changes since the last version
6. Build times remain under 5 minutes with caching
