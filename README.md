# Addness CLI

Addness CLI is a terminal interface for working with Addness from local development environments, scripts, and AI coding agents.

Use it to inspect goals, update progress, write comments, switch organizations, and connect pull requests back to Addness without leaving the command line.

## Features

- Browse and inspect Addness goals from the terminal.
- Update goal status and progress from scripts or local workflows.
- Create comments on goals.
- Link GitHub pull requests to Addness goals.
- Switch between organizations.
- Use machine-readable JSON output for automation.
- Run as a single Rust binary on macOS, Linux, and Windows.

## Installation

macOS and Linux:

```bash
curl -fsSL https://cli.addness.com/install.sh | sh
```

Windows PowerShell:

```powershell
irm https://cli.addness.com/install.ps1 | iex
```

From source:

```bash
git clone https://github.com/AddnessTech/Addness-cli.git
cd Addness-cli
cargo build --release
```

## Login

Run `addness login` and complete the browser-based authentication flow.

## Usage

List goals assigned to you:

```bash
addness goal list --assigned-to me --status NOT_STARTED
```

Use JSON output for scripts and agents:

```bash
addness goal list --assigned-to me --status NOT_STARTED --json
```

Update progress:

```bash
addness goal update <goal-id> --status IN_PROGRESS
addness comment create --goal <goal-id> --body "Implementation started"
```

Link a pull request:

```bash
addness link pr --goal <goal-id> --url https://github.com/org/repo/pull/42
```

Show command help:

```bash
addness --help
addness goal --help
addness org --help
addness comment --help
addness link --help
```

## Development

Addness CLI is written in Rust.

```bash
cargo build
cargo run -- --help
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```

## Contributing

Contributions are welcome through GitHub Pull Requests. Before opening a PR, read [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, review expectations, and merge rules.

Do not include secrets, local settings, customer data, or private screenshots in issues or pull requests.

## Security

Please do not report vulnerabilities through public GitHub issues. See [SECURITY.md](SECURITY.md) for the private reporting process.

## Support

Use GitHub Issues for reproducible bugs, feature requests, and documentation problems. See [SUPPORT.md](SUPPORT.md) for what to include.

## License

Addness CLI is released under the [MIT License](LICENSE).

Copyright (c) 2026 Addness.
