# Contributing

Thanks for contributing to Addness CLI. This repository is maintained as an
open source project, so changes must be reviewed through GitHub Pull Requests.

## Development setup

1. Install the stable Rust toolchain.
2. Clone the repository and create a branch from `main`.
3. Run the local checks before opening a PR:

```bash
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```

## Pull request rules

- Do not push directly to `main`.
- Open a draft PR for work in progress.
- Link the related issue or Addness goal when one exists.
- Keep PRs focused on one behavior change or one maintenance task.
- Include tests or a clear reason why tests are not applicable.
- Document user-visible behavior changes in the PR description.
- Do not include credentials, tokens, private keys, local settings, screenshots
  with private data, or customer data.

## Merge rules

A PR is mergeable only when all of these are true:

- CI is passing.
- At least one maintainer has approved the PR.
- All review conversations are resolved.
- The branch is up to date with `main` when required by GitHub branch protection.
- Security-sensitive changes have been reviewed by a maintainer familiar with
  authentication, credentials, release, or installation flows.

Use squash merge by default. Use a normal merge commit only when preserving a
multi-commit history is intentional and approved by a maintainer. Delete merged
branches after merge.

## Releases

Only maintainers may create release tags. Release tags must use the `vX.Y.Z`
format and should be created from `main` after CI passes.
