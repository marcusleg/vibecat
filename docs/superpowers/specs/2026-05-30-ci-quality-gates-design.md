# CI Quality Gates & Branch Protection

## Overview

Add automated quality gates via GitHub Actions and enforce PR-based workflow on
`main` using GitHub repository rulesets. The goal is to prevent broken code from
landing on `main` while keeping the workflow lightweight for a solo project.

## GitHub Actions Workflow

**File:** `.github/workflows/ci.yml`

**Triggers:**
- Pull requests targeting `main`
- Pushes to `main` (catches post-merge issues if admin bypasses PR)

**Single job (`ci`)** on `ubuntu-latest` with four sequential steps:

| Step | Command | Purpose |
|------|---------|---------|
| Format | `cargo fmt --check` | Reject unformatted code |
| Clippy | `cargo clippy -- -D warnings` | Lint with warnings-as-errors |
| Build | `cargo build` | Verify compilation |
| Test | `cargo test` | Run unit tests + E2E integration tests |

**Actions used (latest versions):**
- `actions/checkout@v6` — clone the repo
- `dtolnay/rust-toolchain@v1` — install stable Rust with `clippy` and `rustfmt` components
- `Swatinem/rust-cache@v2` — cache `target/` and Cargo registry for faster subsequent runs

**Why one job, not four?** This is a small project (~1,200 lines). Four
parallel jobs would each pay the checkout + toolchain + cache overhead
separately. A single job runs all four checks in under a minute with zero
redundancy.

**E2E test CI compatibility:** All 15 E2E tests use loopback only (`127.0.0.1`
/ `::1`). GitHub Actions Ubuntu runners have IPv6 enabled on loopback. No
external network access is needed. Timing margins (200ms sleeps for listener
bind) are generous for CI runners.

## Repository Ruleset

**Method:** GitHub repository rulesets (not legacy branch protection rules).
Created via `gh api repos/{owner}/{repo}/rulesets`.

**Configuration:**

- **Name:** `main-protection`
- **Target:** Default branch (`main`)
- **Enforcement:** `active`
- **Bypass actors:** Repository admins (can merge without passing checks when needed)
- **Rules:**
  1. **Pull request required** (`pull_request`) — no direct pushes to `main`. No approval requirement (solo project).
  2. **Required status checks** (`required_status_checks`) — the `ci` job must pass before merge. Integration branch: `main`.

This means:
- All changes to `main` must go through a pull request
- The `ci` job must pass before a PR can be merged
- Admins can bypass both requirements when needed (e.g., emergency fixes)

## Implementation Steps

1. Create `.github/workflows/ci.yml` with the workflow definition
2. Push the workflow to `main` (needs to exist before rulesets can reference it)
3. Create the repository ruleset via `gh api`
4. Verify: create a test branch, open a PR, confirm checks run and ruleset blocks merge until they pass
