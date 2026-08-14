mod schema;
mod templates;

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::CommandFactory;
use serde::{Deserialize, Serialize};

use crate::Cli;

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub(crate) struct MaterializedCodeApi {
    pub(crate) root: PathBuf,
    pub(crate) operation_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    schema_version: u32,
    cli_version: String,
    content_hash: String,
    operation_count: usize,
}

pub(crate) fn materialize() -> Result<MaterializedCodeApi> {
    let root = dirs::home_dir()
        .map(|home| home.join(".addness").join("code-api"))
        .unwrap_or_else(|| std::env::temp_dir().join("addness-code-api"));
    materialize_at(&root)
}

fn materialize_at(root: &Path) -> Result<MaterializedCodeApi> {
    reject_symlink(root)?;
    fs::create_dir_all(root)
        .with_context(|| format!("Code API root creation failed: {}", root.display()))?;

    let operations = schema::collect(&Cli::command());
    let content_hash = generated_content_hash(&operations)?;
    let manifest = Manifest {
        schema_version: SCHEMA_VERSION,
        cli_version: env!("CARGO_PKG_VERSION").to_string(),
        content_hash,
        operation_count: operations.len(),
    };
    let generated = root.join("generated");
    let manifest_path = generated.join("manifest.json");
    let current = fs::read(&manifest_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Manifest>(&bytes).ok())
        .is_some_and(|stored| stored == manifest)
        && generated.join("_runtime").join("client.mjs").is_file();

    if !current {
        regenerate(root, &generated, &operations, &manifest)?;
    }
    write_if_changed(
        &root.join("README.md"),
        templates::readme(operations.len()).as_bytes(),
    )?;

    Ok(MaterializedCodeApi {
        root: root.to_path_buf(),
        operation_count: operations.len(),
    })
}

fn regenerate(
    root: &Path,
    generated: &Path,
    operations: &[schema::Operation],
    manifest: &Manifest,
) -> Result<()> {
    let temporary = root.join(format!(".generated-tmp-{}", std::process::id()));
    let backup = root.join(format!(".generated-old-{}", std::process::id()));
    remove_path(&temporary)?;
    remove_path(&backup)?;
    fs::create_dir_all(temporary.join("_runtime"))?;
    fs::write(
        temporary.join("_runtime").join("client.mjs"),
        templates::RUNTIME,
    )?;

    for operation in operations {
        let (leaf, parents) = operation
            .command
            .split_last()
            .context("Code API operation had an empty command path")?;
        for component in &operation.command {
            validate_component(component)?;
        }
        let directory = parents
            .iter()
            .fold(temporary.join("addness"), |path, part| path.join(part));
        fs::create_dir_all(&directory)?;
        fs::write(
            directory.join(format!("{leaf}.mjs")),
            templates::module(operation)?,
        )?;
    }
    fs::write(
        temporary.join("manifest.json"),
        serde_json::to_vec_pretty(manifest)?,
    )?;

    if path_exists(generated) {
        fs::rename(generated, &backup)
            .with_context(|| format!("Code API backup failed: {}", generated.display()))?;
    }
    if let Err(error) = fs::rename(&temporary, generated) {
        // 同じschemaを別のTUIプロセスが先に配置した場合は成功として扱う。
        let concurrently_installed = fs::read(generated.join("manifest.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Manifest>(&bytes).ok())
            .is_some_and(|stored| stored == *manifest);
        if concurrently_installed {
            remove_path(&temporary)?;
            remove_path(&backup)?;
            return Ok(());
        }
        if path_exists(&backup) && !path_exists(generated) {
            let _ = fs::rename(&backup, generated);
        }
        let _ = remove_path(&temporary);
        return Err(error).context("Code API atomic replacement failed");
    }
    remove_path(&backup)?;
    Ok(())
}

fn validate_component(component: &str) -> Result<()> {
    if component.is_empty()
        || !component
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        bail!("unsafe Code API path component: {component:?}");
    }
    Ok(())
}

fn reject_symlink(path: &Path) -> Result<()> {
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        bail!("Code API root must not be a symlink: {}", path.display());
    }
    Ok(())
}

fn remove_path(path: &Path) -> Result<()> {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Ok(());
    };
    if metadata.file_type().is_symlink() || metadata.is_file() {
        fs::remove_file(path)?;
    } else if metadata.is_dir() {
        fs::remove_dir_all(path)?;
    }
    Ok(())
}

fn path_exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

fn write_if_changed(path: &Path, content: &[u8]) -> Result<()> {
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        bail!(
            "Code API managed file must not be a symlink: {}",
            path.display()
        );
    }
    if fs::read(path).ok().as_deref() != Some(content) {
        fs::write(path, content)?;
    }
    Ok(())
}

fn generated_content_hash(operations: &[schema::Operation]) -> Result<String> {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    hash = hash_bytes(hash, templates::RUNTIME.as_bytes());
    for operation in operations {
        hash = hash_bytes(hash, templates::module(operation)?.as_bytes());
    }
    Ok(format!("{hash:016x}"))
}

fn hash_bytes(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "addness-code-api-{name}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ))
    }

    #[test]
    fn materialize_writes_progressively_disclosed_modules_without_secrets() {
        let root = test_root("tree");
        let result = materialize_at(&root).expect("materialize code API");
        let goal_get = fs::read_to_string(root.join("generated/addness/goal/get.mjs"))
            .expect("goal get module");
        let runtime =
            fs::read_to_string(root.join("generated/_runtime/client.mjs")).expect("runtime module");

        assert_eq!(result.root, root);
        assert!(result.operation_count > 100);
        assert!(goal_get.contains(r#""command": ["#));
        assert!(goal_get.contains(r#""goal""#));
        assert!(goal_get.contains("export async function run"));
        assert!(runtime.contains("process.env.ADDNESS_BIN"));
        assert!(!runtime.contains("console.log"));
        assert!(!goal_get.contains("ADDNESS_API_TOKEN"));
        remove_path(&root).expect("remove test root");
    }

    #[test]
    fn materialize_replaces_stale_generated_tree_but_keeps_root_files() {
        let root = test_root("replace");
        materialize_at(&root).expect("first materialization");
        fs::write(root.join("keep.mjs"), "user code").expect("root user file");
        fs::write(root.join("generated/stale.mjs"), "stale").expect("stale file");
        fs::write(root.join("generated/manifest.json"), "{}").expect("invalidate manifest");

        materialize_at(&root).expect("second materialization");
        assert_eq!(
            fs::read_to_string(root.join("keep.mjs")).expect("kept root file"),
            "user code"
        );
        assert!(!root.join("generated/stale.mjs").exists());
        remove_path(&root).expect("remove test root");
    }

    #[cfg(unix)]
    #[test]
    fn node_workflow_keeps_intermediate_values_out_of_stdout() {
        use std::os::unix::fs::PermissionsExt;
        use std::process::Command;

        if Command::new("node").arg("--version").output().is_err() {
            return;
        }

        let root = test_root("workflow");
        materialize_at(&root).expect("materialize code API");
        // 空白とshell metacharacterを含む名前でも実行できることを確認し、shell経由に
        // 退行した場合を検出する。
        let fake_addness = root.join("fake addness;literal.mjs");
        fs::write(
            &fake_addness,
            r#"#!/usr/bin/env node
const args = process.argv.slice(2);
if (args[0] === "status") {
  process.stdout.write('{"phase":"one"}\n{"phase":"two"}\n');
} else if (args[0] === "goal" && args[1] === "get") {
  process.stdout.write(JSON.stringify({ id: args[2], secret: "intermediate-secret" }));
} else if (args[0] === "goal" && args[1] === "update") {
  const body = args[args.indexOf("--body") + 1];
  if (body !== "handled:intermediate-secret") process.exit(42);
  process.stdout.write(JSON.stringify({ id: args[2], status: "updated" }));
} else if (args[0] === "execution" && args[1] === "codex" && args[2] === "apply") {
  const changes = args[args.indexOf("--changes-json") + 1];
  if (changes !== '[{"id":"goal-1","secret":"intermediate-secret"}]') process.exit(44);
  process.stdout.write(JSON.stringify({ appliedId: "goal-1" }));
} else {
  process.exit(43);
}
"#,
        )
        .expect("fake addness binary");
        let mut permissions = fs::metadata(&fake_addness)
            .expect("fake binary metadata")
            .permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&fake_addness, permissions).expect("fake binary permissions");

        let workflow = root.join("workflow.mjs");
        fs::write(
            &workflow,
            r#"import assert from "node:assert/strict";
import path from "node:path";
import { pathToFileURL } from "node:url";
const root = process.env.ADDNESS_CODE_API_ROOT;
const load = async (...parts) => import(pathToFileURL(path.join(root, "generated", "addness", ...parts)).href);
const { run: getGoal } = await load("goal", "get.mjs");
const { run: updateGoal } = await load("goal", "update.mjs");
const { run: deleteGoal } = await load("goal", "delete.mjs");
const { run: applyChanges } = await load("execution", "codex", "apply.mjs");
const { run: getStatus } = await load("status.mjs");
await assert.rejects(() => deleteGoal({ id: "goal-1" }), /requires force: true/);
assert.deepEqual(await getStatus(), [{ phase: "one" }, { phase: "two" }]);
const goal = await getGoal({ id: "goal-1" });
const applied = await applyChanges({ changes_json: [{ id: goal.id, secret: goal.secret }] });
const updated = await updateGoal({ id: applied.appliedId, body: `handled:${goal.secret}` });
process.stdout.write(JSON.stringify({ updated: updated.id }));
"#,
        )
        .expect("workflow");

        let output = Command::new("node")
            .arg(&workflow)
            .env("ADDNESS_CODE_API_ROOT", &root)
            .env("ADDNESS_BIN", &fake_addness)
            .output()
            .expect("run workflow");
        assert!(
            output.status.success(),
            "workflow stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8(output.stdout).expect("UTF-8 stdout"),
            r#"{"updated":"goal-1"}"#
        );
        remove_path(&root).expect("remove test root");
    }
}
