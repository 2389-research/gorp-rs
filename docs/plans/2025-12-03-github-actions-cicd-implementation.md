# GitHub Actions CI/CD Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement automated CI/CD system with quality checks on every commit and cross-platform release builds on git tags.

**Architecture:** Two GitHub Actions workflows - ci.yml runs format/lint/test in parallel on all pushes and PRs, release.yml builds Linux and macOS binaries and creates GitHub releases when v* tags are pushed. Both use aggressive caching with Cargo.lock-based invalidation.

**Tech Stack:** GitHub Actions, Rust stable toolchain, actions/cache for caching, actions/checkout for code checkout

---

## Task 1: Create GitHub Workflows Directory

**Files:**
- Create: `.github/workflows/`

**Step 1: Create the directory**

```bash
mkdir -p .github/workflows
```

**Step 2: Verify directory exists**

```bash
ls -la .github/
```

Expected: Directory `.github/workflows/` exists

**Step 3: Commit**

```bash
git add .github/workflows/.gitkeep
touch .github/workflows/.gitkeep
git add .github/workflows/.gitkeep
git commit -m "chore: create GitHub workflows directory"
```

---

## Task 2: Implement CI Workflow

**Files:**
- Create: `.github/workflows/ci.yml`

**Step 1: Create CI workflow file**

Create `.github/workflows/ci.yml` with complete configuration:

```yaml
name: CI

on:
  push:
    branches: [ "*" ]
  pull_request:
    branches: [ "*" ]

jobs:
  format:
    name: Format Check
    runs-on: ubuntu-latest
    steps:
      - name: Checkout code
        uses: actions/checkout@v4

      - name: Install Rust stable
        uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt

      - name: Run rustfmt check
        run: cargo fmt --all -- --check

  clippy:
    name: Clippy Lint
    runs-on: ubuntu-latest
    steps:
      - name: Checkout code
        uses: actions/checkout@v4

      - name: Install Rust stable
        uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy

      - name: Cache cargo registry
        uses: actions/cache@v4
        with:
          path: |
            ~/.cargo/bin/
            ~/.cargo/registry/index/
            ~/.cargo/registry/cache/
            ~/.cargo/git/db/
            target/
          key: rust-cache-${{ runner.os }}-${{ hashFiles('**/Cargo.lock') }}
          restore-keys: |
            rust-cache-${{ runner.os }}-

      - name: Run clippy
        run: cargo clippy --all-targets --all-features -- -D warnings

  test:
    name: Test Suite
    runs-on: ubuntu-latest
    steps:
      - name: Checkout code
        uses: actions/checkout@v4

      - name: Install Rust stable
        uses: dtolnay/rust-toolchain@stable

      - name: Cache cargo registry
        uses: actions/cache@v4
        with:
          path: |
            ~/.cargo/bin/
            ~/.cargo/registry/index/
            ~/.cargo/registry/cache/
            ~/.cargo/git/db/
            target/
          key: rust-cache-${{ runner.os }}-${{ hashFiles('**/Cargo.lock') }}
          restore-keys: |
            rust-cache-${{ runner.os }}-

      - name: Run tests
        run: cargo test --all-features
```

**Step 2: Verify YAML syntax**

```bash
cat .github/workflows/ci.yml
```

Expected: File contents display correctly, no syntax errors visible

**Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "feat(ci): add CI workflow with format, lint, and test jobs

- Three parallel jobs for fast feedback
- Format check with cargo fmt
- Clippy linting with warnings as errors
- Test suite execution
- Aggressive caching with Cargo.lock invalidation"
```

---

## Task 3: Implement Release Workflow

**Files:**
- Create: `.github/workflows/release.yml`

**Step 1: Create release workflow file**

Create `.github/workflows/release.yml` with complete configuration:

```yaml
name: Release

on:
  push:
    tags:
      - 'v*'

jobs:
  build:
    name: Build ${{ matrix.platform }}
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        include:
          - os: ubuntu-latest
            target: x86_64-unknown-linux-gnu
            platform: Linux x86_64
            artifact_name: matrix-bridge-linux-x86_64
          - os: macos-latest
            target: x86_64-apple-darwin
            platform: macOS x86_64
            artifact_name: matrix-bridge-macos-x86_64

    steps:
      - name: Checkout code
        uses: actions/checkout@v4
        with:
          fetch-depth: 0

      - name: Install Rust stable
        uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}

      - name: Cache cargo registry
        uses: actions/cache@v4
        with:
          path: |
            ~/.cargo/bin/
            ~/.cargo/registry/index/
            ~/.cargo/registry/cache/
            ~/.cargo/git/db/
            target/
          key: rust-cache-${{ matrix.os }}-${{ hashFiles('**/Cargo.lock') }}
          restore-keys: |
            rust-cache-${{ matrix.os }}-

      - name: Build release binary
        run: cargo build --release --target ${{ matrix.target }}

      - name: Strip binary (Linux)
        if: matrix.os == 'ubuntu-latest'
        run: strip target/${{ matrix.target }}/release/matrix-bridge

      - name: Strip binary (macOS)
        if: matrix.os == 'macos-latest'
        run: strip target/${{ matrix.target }}/release/matrix-bridge

      - name: Rename binary
        run: |
          cp target/${{ matrix.target }}/release/matrix-bridge ${{ matrix.artifact_name }}

      - name: Upload artifact
        uses: actions/upload-artifact@v4
        with:
          name: ${{ matrix.artifact_name }}
          path: ${{ matrix.artifact_name }}
          if-no-files-found: error

  release:
    name: Create Release
    needs: build
    runs-on: ubuntu-latest
    permissions:
      contents: write

    steps:
      - name: Checkout code
        uses: actions/checkout@v4
        with:
          fetch-depth: 0

      - name: Download Linux artifact
        uses: actions/download-artifact@v4
        with:
          name: matrix-bridge-linux-x86_64
          path: ./artifacts

      - name: Download macOS artifact
        uses: actions/download-artifact@v4
        with:
          name: matrix-bridge-macos-x86_64
          path: ./artifacts

      - name: Generate changelog
        id: changelog
        run: |
          # Get previous tag
          PREV_TAG=$(git tag --sort=-v:refname | grep -v "^${GITHUB_REF_NAME}$" | head -n 1)

          if [ -z "$PREV_TAG" ]; then
            echo "No previous tag found, using all commits"
            COMMITS=$(git log --pretty=format:"- %s" $GITHUB_REF_NAME)
          else
            echo "Generating changelog from $PREV_TAG to $GITHUB_REF_NAME"
            COMMITS=$(git log --pretty=format:"- %s" $PREV_TAG..$GITHUB_REF_NAME)
          fi

          # Group by conventional commit type
          FEATURES=$(echo "$COMMITS" | grep "^- feat:" || true)
          FIXES=$(echo "$COMMITS" | grep "^- fix:" || true)
          DOCS=$(echo "$COMMITS" | grep "^- docs:" || true)
          CHORES=$(echo "$COMMITS" | grep "^- chore:" || true)

          # Build changelog
          CHANGELOG="## Changes in $GITHUB_REF_NAME"

          if [ -n "$FEATURES" ]; then
            CHANGELOG="$CHANGELOG\n\n### Features\n$FEATURES"
          fi

          if [ -n "$FIXES" ]; then
            CHANGELOG="$CHANGELOG\n\n### Bug Fixes\n$FIXES"
          fi

          if [ -n "$DOCS" ]; then
            CHANGELOG="$CHANGELOG\n\n### Documentation\n$DOCS"
          fi

          if [ -n "$CHORES" ]; then
            CHANGELOG="$CHANGELOG\n\n### Chores\n$CHORES"
          fi

          # Save to file and output
          echo -e "$CHANGELOG" > changelog.md
          echo "changelog<<EOF" >> $GITHUB_OUTPUT
          cat changelog.md >> $GITHUB_OUTPUT
          echo "EOF" >> $GITHUB_OUTPUT

      - name: Create GitHub Release
        uses: softprops/action-gh-release@v1
        with:
          body: ${{ steps.changelog.outputs.changelog }}
          files: |
            ./artifacts/matrix-bridge-linux-x86_64
            ./artifacts/matrix-bridge-macos-x86_64
          fail_on_unmatched_files: true
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
```

**Step 2: Verify YAML syntax**

```bash
cat .github/workflows/release.yml
```

Expected: File contents display correctly, no syntax errors visible

**Step 3: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "feat(ci): add release workflow with cross-platform builds

- Matrix builds for Linux x86_64 and macOS x86_64
- Automated binary stripping to reduce size
- Auto-generated changelog from conventional commits
- GitHub release creation with binaries attached
- Triggered on v* tags"
```

---

## Task 4: Add Dependabot Configuration (Optional)

**Files:**
- Create: `.github/dependabot.yml`

**Step 1: Create dependabot config**

Create `.github/dependabot.yml`:

```yaml
version: 2
updates:
  # Keep Rust dependencies up to date
  - package-ecosystem: "cargo"
    directory: "/"
    schedule:
      interval: "weekly"
    open-pull-requests-limit: 10

  # Keep GitHub Actions up to date
  - package-ecosystem: "github-actions"
    directory: "/"
    schedule:
      interval: "weekly"
    open-pull-requests-limit: 5
```

**Step 2: Verify YAML syntax**

```bash
cat .github/dependabot.yml
```

Expected: File contents display correctly

**Step 3: Commit**

```bash
git add .github/dependabot.yml
git commit -m "chore(ci): add Dependabot configuration

- Weekly Cargo dependency updates
- Weekly GitHub Actions updates
- Controlled PR limits to avoid noise"
```

---

## Task 5: Verify CI Workflow Locally

**Files:**
- None (verification only)

**Step 1: Run format check**

```bash
cargo fmt --all -- --check
```

Expected: `Diff in /path/to/file.rs at line X` or no output if formatted

**Step 2: Run clippy**

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: `Finished dev [unoptimized + debuginfo] target(s)` with 0 warnings

**Step 3: Run tests**

```bash
cargo test --all-features
```

Expected: `test result: ok. 8 passed; 0 failed`

**Step 4: Verify all checks pass**

All three commands above should succeed. If any fail, fix the issues before pushing.

---

## Task 6: Push and Verify CI Workflow on GitHub

**Files:**
- None (verification only)

**Step 1: Push to remote**

```bash
git push origin main
```

Expected: Push succeeds

**Step 2: Check GitHub Actions tab**

Navigate to repository on GitHub → Actions tab

Expected: CI workflow triggered and running with three parallel jobs

**Step 3: Verify all jobs pass**

Wait for workflow to complete

Expected: All three jobs (format, clippy, test) show green checkmarks

---

## Task 7: Test Release Workflow with Test Tag

**Files:**
- None (verification only)

**Step 1: Create test tag**

```bash
git tag v0.1.1-test
git push origin v0.1.1-test
```

Expected: Tag pushed successfully

**Step 2: Check GitHub Actions**

Navigate to repository → Actions tab

Expected: Release workflow triggered

**Step 3: Verify workflow completes**

Wait for both build jobs and release job to complete

Expected:
- Both platform builds succeed
- Release created under Releases tab
- Two binaries attached to release
- Changelog generated from commits

**Step 4: Verify binaries (optional)**

Download both binaries from release and verify they execute on respective platforms

Expected: Binaries run successfully

**Step 5: Clean up test release**

Delete the test release and tag from GitHub UI

Expected: Test artifacts cleaned up

---

## Task 8: Update README with CI Badge

**Files:**
- Modify: `README.md`

**Step 1: Add CI badge to README**

Add after the title in `README.md`:

```markdown
# Matrix-Claude Bridge

[![CI](https://github.com/YOUR_USERNAME/YOUR_REPO/actions/workflows/ci.yml/badge.svg)](https://github.com/YOUR_USERNAME/YOUR_REPO/actions/workflows/ci.yml)
```

Replace `YOUR_USERNAME` and `YOUR_REPO` with actual values.

**Step 2: Verify badge URL**

Open README on GitHub and verify badge displays correctly

Expected: Green badge showing "CI passing"

**Step 3: Commit**

```bash
git add README.md
git commit -m "docs: add CI status badge to README"
```

---

## Post-Implementation Notes

### Testing the Full Release Flow

To create a real release:

```bash
git tag v0.2.0
git push origin v0.2.0
```

This will trigger the release workflow and create a production release.

### Branch Protection Setup

Configure on GitHub (Settings → Branches → Add rule for `main`):
- Require status checks to pass before merging
- Select: `Format Check`, `Clippy Lint`, `Test Suite`
- Require branches to be up to date before merging

### Monitoring

- Check Actions tab regularly for workflow status
- Review Dependabot PRs weekly
- Update workflows if GitHub Actions deprecate features

### Troubleshooting

**CI fails on format:**
```bash
cargo fmt --all
git commit -am "style: format code"
```

**CI fails on clippy:**
```bash
cargo clippy --all-targets --all-features --fix --allow-dirty
git commit -am "fix: address clippy warnings"
```

**Release build fails:**
- Check Cargo.toml for platform-specific dependencies
- Verify no platform-specific code breaks cross-compilation
- Review GitHub Actions logs for specific error

### Future Enhancements

See design document section "Future Enhancements" for potential additions:
- ARM64 builds
- Security audits
- Performance benchmarking
- Docker image builds
