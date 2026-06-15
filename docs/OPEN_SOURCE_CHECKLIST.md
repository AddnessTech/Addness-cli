# Open source readiness checklist

This checklist covers repository settings that cannot be fully enforced by
files in the repository.

## Before making the repository public

- Confirm the intended license is Apache-2.0.
- Confirm Addness trademark and Terms of Service language is present.
- Remove local-only files and unrelated assets from the repository.
- Run a secret scan against the full Git history.
- Rotate any credential that has ever been committed.
- Confirm release infrastructure variables and secrets are stored only in
  GitHub repository or organization secrets/variables.
- Confirm all third-party dependencies are acceptable for public distribution.

## GitHub repository settings

Enable these settings before accepting external contributions:

- Require Pull Requests before merging to `main`.
- Require at least one approving review.
- Dismiss stale approvals when new commits are pushed.
- Require all conversations to be resolved before merge.
- Require the `CI / check` status check.
- Require the `Security / cargo audit` status check.
- Block force pushes to `main`.
- Block deletion of `main`.
- Enable squash merge.
- Disable direct merge commits unless maintainers explicitly want them.
- Enable automatically delete head branches.
- Enable Dependabot alerts.
- Enable Dependabot security updates.
- Enable secret scanning and push protection.
- Enable private vulnerability reporting.

## Merge policy

- All code changes must enter through a PR.
- Maintainers merge after checks pass and review is complete.
- Security-sensitive changes require maintainer review even if they are small.
- Releases are tagged from `main` only after CI passes.
