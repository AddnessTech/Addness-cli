use clap::{ArgAction, Command};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Operation {
    pub(super) command: Vec<String>,
    pub(super) description: String,
    pub(super) parameters: Vec<Parameter>,
    pub(super) requires_force: bool,
}

impl Operation {
    pub(super) fn id(&self) -> String {
        self.command.join("/")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Parameter {
    pub(super) name: String,
    pub(super) kind: ParameterKind,
    pub(super) flag: Option<String>,
    pub(super) required: bool,
    pub(super) description: String,
    pub(super) value_names: Vec<String>,
    pub(super) possible_values: Vec<String>,
    pub(super) default_values: Vec<String>,
    pub(super) json_value: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum ParameterKind {
    Positional,
    PositionalMany,
    Option,
    Append,
    Flag,
    NegatedFlag,
    Count,
}

pub(super) fn collect(root: &Command) -> Vec<Operation> {
    let mut operations = Vec::new();
    collect_from(root, &mut Vec::new(), &mut operations);
    operations.sort_by_key(Operation::id);
    operations
}

fn collect_from(command: &Command, path: &mut Vec<String>, operations: &mut Vec<Operation>) {
    for subcommand in command.get_subcommands() {
        if subcommand.is_hide_set() {
            continue;
        }
        path.push(subcommand.get_name().to_string());
        if subcommand.has_subcommands() {
            collect_from(subcommand, path, operations);
        } else if supports_structured_output(subcommand) {
            operations.push(operation_from(subcommand, path));
        }
        path.pop();
    }
}

fn supports_structured_output(command: &Command) -> bool {
    command
        .get_arguments()
        .any(|argument| argument.get_id().as_str() == "json")
}

fn operation_from(command: &Command, path: &[String]) -> Operation {
    let mut parameters = command
        .get_arguments()
        .filter(|argument| {
            let id = argument.get_id().as_str();
            id != "help" && id != "version" && id != "json" && !argument.is_hide_set()
        })
        .map(|argument| {
            let name = argument.get_id().as_str().to_string();
            // clap derive で明示 index のない positional は command tree の build 前だと
            // get_index() が None のことがある。flag 名の有無は build 前でも確定している。
            let positional = argument.get_long().is_none() && argument.get_short().is_none();
            let kind = if positional {
                if matches!(argument.get_action(), ArgAction::Append) {
                    ParameterKind::PositionalMany
                } else {
                    ParameterKind::Positional
                }
            } else {
                match argument.get_action() {
                    ArgAction::SetTrue => ParameterKind::Flag,
                    ArgAction::SetFalse => ParameterKind::NegatedFlag,
                    ArgAction::Append => ParameterKind::Append,
                    ArgAction::Count => ParameterKind::Count,
                    _ => ParameterKind::Option,
                }
            };
            let flag = if positional {
                None
            } else if let Some(long) = argument.get_long() {
                Some(format!("--{long}"))
            } else {
                argument.get_short().map(|short| format!("-{short}"))
            };
            let default_values = argument
                .get_default_values()
                .iter()
                .map(|value| value.to_string_lossy().into_owned())
                .collect();
            let value_names = argument
                .get_value_names()
                .into_iter()
                .flatten()
                .map(ToString::to_string)
                .collect();
            let possible_values = argument
                .get_possible_values()
                .into_iter()
                .filter(|value| !value.is_hide_set())
                .map(|value| value.get_name().to_string())
                .collect();
            Parameter {
                json_value: name.ends_with("_json"),
                name,
                kind,
                flag,
                required: argument.is_required_set(),
                description: argument
                    .get_help()
                    .map(ToString::to_string)
                    .unwrap_or_default(),
                value_names,
                possible_values,
                default_values,
            }
        })
        .collect::<Vec<_>>();
    parameters.sort_by_key(|parameter| match parameter.kind {
        ParameterKind::Positional | ParameterKind::PositionalMany => 0,
        _ => 1,
    });

    Operation {
        command: path.to_vec(),
        description: command
            .get_about()
            .map(ToString::to_string)
            .unwrap_or_default(),
        requires_force: parameters.iter().any(|parameter| parameter.name == "force"),
        parameters,
    }
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;
    use crate::Cli;

    #[test]
    fn collect_exposes_json_commands_without_json_parameter() {
        let operations = collect(&Cli::command());
        let get = operations
            .iter()
            .find(|operation| operation.command == ["goal", "get"])
            .expect("goal/get operation");

        assert!(get.parameters.iter().any(|parameter| parameter.name == "id"
            && parameter.kind == ParameterKind::Positional
            && parameter.flag.is_none()));
        assert!(
            get.parameters
                .iter()
                .all(|parameter| parameter.name != "json")
        );

        let notification = operations
            .iter()
            .find(|operation| operation.command == ["notification", "send"])
            .expect("notification/send operation");
        assert!(notification.parameters.iter().any(|parameter| {
            parameter.name == "kind"
                && parameter
                    .possible_values
                    .iter()
                    .any(|value| value == "done")
        }));
    }

    #[test]
    fn collect_omits_interactive_and_unstructured_commands() {
        let ids = collect(&Cli::command())
            .into_iter()
            .map(|operation| operation.id())
            .collect::<Vec<_>>();

        assert!(!ids.iter().any(|id| id == "login"));
        assert!(!ids.iter().any(|id| id == "configure"));
        assert!(!ids.iter().any(|id| id == "completions"));
        assert!(ids.iter().any(|id| id == "search"));
    }

    #[test]
    fn collect_marks_native_json_values_and_force_guards() {
        let operations = collect(&Cli::command());
        let apply = operations
            .iter()
            .find(|operation| operation.command == ["execution", "codex", "apply"])
            .expect("execution/codex/apply operation");
        assert!(
            apply
                .parameters
                .iter()
                .any(|parameter| parameter.name == "changes_json" && parameter.json_value)
        );

        let delete = operations
            .iter()
            .find(|operation| operation.command == ["goal", "delete"])
            .expect("goal/delete operation");
        assert!(delete.requires_force);
    }
}
