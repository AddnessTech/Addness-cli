use anyhow::{Context, Result};

use super::schema::{Operation, Parameter, ParameterKind};

pub(super) const RUNTIME: &str = include_str!("runtime.mjs");

pub(super) fn module(operation: &Operation) -> Result<String> {
    let depth = operation.command.len();
    let runtime_path = format!("{}_runtime/client.mjs", "../".repeat(depth));
    let definition = serde_json::to_string_pretty(operation)
        .context("Code API operation definition serialization failed")?;
    let description = jsdoc_text(&operation.description);
    let parameters = operation
        .parameters
        .iter()
        .map(jsdoc_parameter)
        .collect::<Vec<_>>()
        .join("\n");
    let input_type = if parameters.is_empty() {
        " * @param {Object} [input]".to_string()
    } else {
        format!(" * @param {{Object}} input\n{parameters}")
    };

    Ok(format!(
        r#"import {{ callAddness }} from "{runtime_path}";

export const definition = Object.freeze({definition});

/**
 * {description}
{input_type}
 * @param {{{{ cwd?: string, timeoutMs?: number, maxOutputBytes?: number, signal?: AbortSignal }}}} [options]
 * @returns {{Promise<unknown>}}
 */
export async function run(input = {{}}, options = {{}}) {{
  return callAddness(definition, input, options);
}}

export default run;
"#
    ))
}

fn jsdoc_parameter(parameter: &Parameter) -> String {
    let optional = if parameter.required { "" } else { "[" };
    let optional_end = if parameter.required { "" } else { "]" };
    let default = if parameter.default_values.is_empty() {
        String::new()
    } else {
        format!(
            " Default: {}.",
            jsdoc_text(&parameter.default_values.join(", "))
        )
    };
    let allowed = if parameter.possible_values.is_empty() {
        String::new()
    } else {
        format!(
            " Allowed: {}.",
            jsdoc_text(&parameter.possible_values.join(" | "))
        )
    };
    let description = jsdoc_text(&parameter.description);
    format!(
        " * @property {{{}}} {optional}{}{optional_end} - {description}{allowed}{default}",
        js_type(parameter),
        parameter.name
    )
}

fn js_type(parameter: &Parameter) -> &'static str {
    if parameter.json_value {
        return "unknown";
    }
    match parameter.kind {
        ParameterKind::Flag | ParameterKind::NegatedFlag => "boolean",
        ParameterKind::Count => "number",
        ParameterKind::Append | ParameterKind::PositionalMany => "Array<string | number>",
        ParameterKind::Option | ParameterKind::Positional => "string | number | boolean",
    }
}

fn jsdoc_text(text: &str) -> String {
    let text = text.trim().replace("*/", "* /");
    if text.is_empty() {
        return "Addness operation.".to_string();
    }
    text.replace('\n', " ")
}

pub(super) fn readme(operation_count: usize) -> String {
    format!(
        r#"# Addness Code API

This directory exposes {operation_count} Addness operations as filesystem code APIs.
Definitions are generated from the CLI schema and contain no credentials.

## Progressive discovery

1. List `generated/addness/` to find a domain.
2. Search names/descriptions with `rg -l '<keyword>' generated/addness --glob '*.mjs'`.
3. Read only the operation modules needed for the task.
4. Import their `run()` functions into one Node.js `.mjs` program.
5. Keep intermediate values in memory. Write only the minimal final result to stdout.

Do not load `manifest.json` or every module into model context. The filesystem is the catalog.

## Example

```js
import path from "node:path";
import {{ pathToFileURL }} from "node:url";

const operation = (...parts) => pathToFileURL(path.join(
  process.env.ADDNESS_CODE_API_ROOT,
  "generated",
  "addness",
  ...parts,
)).href;
const {{ run: getGoal }} = await import(operation("goal", "get.mjs"));
const {{ run: updateGoal }} = await import(operation("goal", "update.mjs"));

const goal = await getGoal({{ id: process.env.ADDNESS_GOAL_ID }});
await updateGoal({{ id: goal.id, body: goal.body }});
process.stdout.write(JSON.stringify({{ updated: goal.id }}));
```

Run the program with Node.js. `run()` starts the current `ADDNESS_BIN` directly without a shell,
forces structured JSON output, parses JSON/JSONL in-process, and returns the value without logging it.
Operations with a `--force` guard require `force: true` and never open an interactive prompt.
Native objects passed to parameters ending in `_json` are serialized automatically.
"#
    )
}
