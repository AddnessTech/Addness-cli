# Today Command Release Notes

## Added

- Added `addness today` to list today's todos from the CLI.
- Added `addness today add --title "..."` to create a goal for today's work.
- Added `addness today done <ID>`, `addness today reopen <ID>`, and
  `addness today status <ID> <STATUS>` to update today's todo state from the CLI.
- Added support for `--json`, `--org`, and `--date YYYY-MM-DD` where applicable.

## Changed

- Human-readable today lists show completion state, colored status, goal ID, and
  hierarchy indentation.
- Today status updates use the same status parsing as the existing `goal`
  command.

## Notes

- `addness today` without a subcommand behaves like `addness today list`.
- `today add` uses the normal goal creation API. Goals are created under the
  specified parent, or as root goals when no parent is specified.
