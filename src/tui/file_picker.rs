use std::path::PathBuf;

use super::app::DeliverableKind;

/// ファイラーで選んだパスを書き戻す先のモーダル状態。
/// キャンセル時は元の値で、ファイル選択時は選んだパスで復元する。
#[derive(Debug, Clone)]
pub(super) enum FilePickerReturn {
    AddDeliverable {
        goal_id: String,
        goal_title: String,
        kind: DeliverableKind,
        name: String,
        value: String,
    },
    UpdateDeliverable {
        goal_id: String,
        deliverable_id: String,
        deliverable_name: String,
        content_file: String,
    },
}

/// ファイラーの1エントリ。
#[derive(Debug, Clone)]
pub(super) struct FileEntry {
    pub name: String,
    pub is_dir: bool,
}

/// ファイラーで一度に表示する行数（スクロール計算と描画で共有）。
pub(super) const PICKER_VISIBLE_ROWS: usize = 12;

/// 先頭の `~` をホームディレクトリに展開する。
pub(super) fn expand_tilde(input: &str) -> String {
    if let Some(rest) = input.strip_prefix('~')
        && (rest.is_empty() || rest.starts_with('/'))
        && let Some(home) = dirs::home_dir()
    {
        return format!("{}{}", home.display(), rest);
    }
    input.to_string()
}

/// パス入力をファイルシステムから補完する（共通接頭辞まで）。
/// 補完候補が無ければ None。候補が1つでディレクトリなら末尾に `/` を付ける。
pub(super) fn complete_path(input: &str) -> Option<String> {
    let expanded = expand_tilde(input);
    let path = std::path::Path::new(&expanded);

    let (dir, prefix) = if expanded.ends_with('/') {
        (PathBuf::from(expanded.trim_end_matches('/')), String::new())
    } else {
        let parent = path.parent().map(|p| p.to_path_buf());
        let dir = match parent {
            Some(p) if p.as_os_str().is_empty() => PathBuf::from("."),
            Some(p) => p,
            None => PathBuf::from("."),
        };
        let prefix = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        (dir, prefix)
    };

    let mut names: Vec<(String, bool)> = std::fs::read_dir(&dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            if name.starts_with(&prefix) {
                Some((name, e.path().is_dir()))
            } else {
                None
            }
        })
        .collect();
    if names.is_empty() {
        return None;
    }
    names.sort();

    let lcp = longest_common_prefix(names.iter().map(|(n, _)| n.as_str()));
    let single_dir = names.len() == 1 && names[0].1;

    // 入力にディレクトリ区切りが無い場合は元の見た目（カレント相対）を保つ。
    let mut result = if expanded.contains('/') {
        let mut p = dir.join(&lcp).to_string_lossy().into_owned();
        if expanded.ends_with('/') && lcp.is_empty() {
            p = dir.to_string_lossy().into_owned();
        }
        p
    } else {
        lcp
    };
    if single_dir {
        result.push('/');
    }
    Some(result)
}

/// 文字列群の最長共通接頭辞。
pub(super) fn longest_common_prefix<'a>(mut iter: impl Iterator<Item = &'a str>) -> String {
    let Some(first) = iter.next() else {
        return String::new();
    };
    let mut prefix: Vec<char> = first.chars().collect();
    for s in iter {
        let common = prefix
            .iter()
            .zip(s.chars())
            .take_while(|(a, b)| **a == *b)
            .count();
        prefix.truncate(common);
        if prefix.is_empty() {
            break;
        }
    }
    prefix.into_iter().collect()
}

/// ファイラーの初期ディレクトリを現在の入力値から決める。
pub(super) fn initial_picker_dir(current: &str) -> PathBuf {
    let trimmed = current.trim();
    if !trimmed.is_empty() {
        let expanded = expand_tilde(trimmed);
        let p = PathBuf::from(&expanded);
        if p.is_dir() {
            return p;
        }
        if let Some(parent) = p.parent()
            && parent.is_dir()
            && !parent.as_os_str().is_empty()
        {
            return parent.to_path_buf();
        }
    }
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// ディレクトリの中身を読み、ディレクトリ→ファイルの順、各々名前順で返す。
/// 隠しファイル（.始まり）は除外する。
pub(super) fn read_dir_entries(dir: &std::path::Path) -> Vec<FileEntry> {
    let mut entries: Vec<FileEntry> = match std::fs::read_dir(dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().into_string().ok()?;
                if name.starts_with('.') {
                    return None;
                }
                let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                Some(FileEntry { name, is_dir })
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });
    entries
}
