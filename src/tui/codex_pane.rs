//! TUI 内に codex を擬似端末（PTY）として埋め込むためのモジュール。
//!
//! codex 自体がフルスクリーンの対話型 TUI なので、PTY 上で起動し、その VT 出力を
//! vt100 でパースして ratatui のペインに描画、キー入力を PTY へ転送する。
//! codex は同梱の `addness` CLI を通じて Addness（タスク DB）を読み書きする想定で、
//! 起動時に対象ゴールの文脈をプロンプトとして注入する。

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Instant;

use anyhow::{Context, Result};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tui_term::vt100;

/// codex 実行ファイルのパスを解決する。
/// 環境変数 `ADDNESS_CODEX_BIN` を最優先で見て、無ければ PATH 上を探す。
/// 見つからなければ `None`。
pub fn codex_path() -> Option<PathBuf> {
    // 明示指定（別パスにインストールした場合や検証用の上書き）を最優先。
    if let Some(bin) = std::env::var_os("ADDNESS_CODEX_BIN") {
        let cand = PathBuf::from(bin);
        if cand.is_file() {
            return Some(cand);
        }
    }
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let cand = dir.join("codex");
        if cand.is_file() {
            return Some(cand);
        }
    }
    None
}

/// 作業ディレクトリの `git diff --stat`（HEAD 比較）を取得する。
/// 還流コメントのプリフィルに使う。取得できなければ空文字。
pub fn git_diff_stat(cwd: &Path) -> String {
    let output = std::process::Command::new("git")
        .args(["diff", "--stat", "HEAD"])
        .current_dir(cwd)
        .output();
    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => String::new(),
    }
}

/// 埋め込み codex セッションの状態。
pub struct CodexPane {
    parser: vt100::Parser,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    rx: Receiver<Vec<u8>>,
    /// codex プロセスが終了済みか。
    pub finished: bool,
    rows: u16,
    cols: u16,
    /// 還流先となる対象ゴールの ID。
    pub goal_id: String,
    /// 契約ペイン表示用に保持する対象ゴールのタイトルと DoD。
    pub goal_title: String,
    pub dod: String,
    /// codex が参照しているローカルの作業ディレクトリ（cwd）。
    pub cwd: String,
    /// 対象ゴールの現在ステータス表示（例: "進行中"）。
    pub status_label: String,
    /// DoD を行単位に分割した項目（契約ペインのチェックリスト用）。
    pub dod_items: Vec<String>,
    /// 各 DoD 項目の達成判定。None=未判定 / Some(true)=達成 / Some(false)=未達。
    pub dod_checks: Vec<Option<bool>>,
    /// DoD 自動判定（codex exec）を実行中か。
    pub assessing: bool,
    /// 子ゴール数・コメント数（変化検知で更新ログに反映する。未取得は None）。
    pub child_count: Option<usize>,
    pub comment_count: Option<usize>,
    /// 子ゴールのライブリスト（新着は new_until までハイライト）。
    pub children: Vec<ChildGoal>,
    /// codex が直近に実行した addness 操作の表示ラベル（参照/書込中インジケータ）。
    pub action: Option<String>,
    /// ステータス・DoD が変化した時刻（変化行を数秒ハイライトするのに使う）。
    pub status_changed_at: Option<Instant>,
    pub dod_changed_at: Option<Instant>,
    /// codex ログのスクロールバック位置（0=最新、増えるほど過去）。
    pub scrollback: usize,
    /// Addness 側の更新ログ（codex の書き込みやステータス変化を可視化）。新しいものほど末尾。
    pub activity: Vec<String>,
    /// codex セッション終了時の Addness body 自動記録を試行済みか。
    pub auto_record_attempted: bool,
    input_state: CodexInputState,
}

/// 子ゴール 1 件の表示用情報。
pub struct ChildGoal {
    pub id: String,
    pub title: String,
    pub icon: &'static str,
    /// 新着ハイライトの有効期限（None=通常表示）。
    pub new_until: Option<Instant>,
}

/// `addness <サブコマンド…>` 文字列を「いま何をしているか」の表示ラベルへ変換する。
fn action_label(rest: &str) -> String {
    let mut it = rest.split_whitespace();
    let a = it.next().unwrap_or("");
    let b = it.next().unwrap_or("");
    match (a, b) {
        ("goal", "create") => "子ゴールを作成中".to_string(),
        ("goal", "update") => "ゴールを更新中".to_string(),
        ("goal", "get" | "list" | "children" | "tree" | "search" | "siblings") => {
            "ゴールを参照中".to_string()
        }
        ("comment", "create") => "コメントを書込中".to_string(),
        ("comment", _) => "コメントを参照中".to_string(),
        ("link" | "deliverable", _) => "成果物を登録中".to_string(),
        ("today", _) => "今日のtodoを更新中".to_string(),
        ("status" | "summary", _) => "状況を確認中".to_string(),
        (cmd, _) if !cmd.is_empty() => format!("addness {cmd} 実行中"),
        _ => "addness を実行中".to_string(),
    }
}

/// DoD テキストを行単位の項目リストへ分割する（空行は除外）。
fn split_dod_items(dod: &str) -> Vec<String> {
    dod.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect()
}

impl CodexPane {
    /// codex を PTY 上で起動する。
    /// 対象ゴールの文脈は環境変数で渡し、codex の初期プロンプト引数として
    /// Addness CLI での想起指示を渡す。
    pub fn spawn(
        codex_bin: &Path,
        cwd: &Path,
        addness_bin: &str,
        goal_id: String,
        goal_title: String,
        dod: String,
        status_label: String,
    ) -> Result<Self> {
        let rows: u16 = 24;
        let cols: u16 = 80;
        // DoD 項目は一度だけ分割し、チェック状態はその件数で初期化する。
        let dod_items = split_dod_items(&dod);
        let dod_checks = vec![None; dod_items.len()];

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("PTY の確保に失敗しました")?;

        let mut cmd = CommandBuilder::new(codex_bin);
        // 長い運用ルールはチャットに残さず developer_instructions として渡す。
        cmd.arg("-c");
        cmd.arg(codex_config_arg(
            CodexConfigKey::DeveloperInstructions,
            addness_tui_developer_instructions(),
        ));
        // デフォルトでは初期プロンプトを送らず、ユーザーが即入力できる状態で起動する。
        // 完全な Addness 読込は developer instructions に従って必要時に遅延実行する。
        if eager_startup_recall_enabled() {
            cmd.arg(startup_recall_prompt());
        }
        cmd.cwd(cwd);
        // 親プロセスの環境を引き継ぐ（PATH 等を codex のサブプロセスに渡すため）。
        // vars() は非UTF-8な環境変数があると panic するため vars_os() を使う。
        for (key, value) in std::env::vars_os() {
            cmd.env(key, value);
        }
        cmd.env("TERM", "xterm-256color");
        // 対象ゴールの文脈を環境変数で渡す（AGENTS.md がこれを使って想起を指示する）。
        cmd.env("ADDNESS_TUI_CODEX", "1");
        cmd.env("ADDNESS_GOAL_ID", &goal_id);
        cmd.env("ADDNESS_GOAL_TITLE", &goal_title);
        cmd.env("ADDNESS_GOAL_STATUS", &status_label);
        cmd.env("ADDNESS_GOAL_DOD", &dod);
        cmd.env("ADDNESS_BIN", addness_bin);

        let child = pair
            .slave
            .spawn_command(cmd)
            .context("codex の起動に失敗しました")?;
        // slave を閉じておかないと、子プロセス終了時に reader が EOF を受け取れない。
        drop(pair.slave);

        let reader = pair
            .master
            .try_clone_reader()
            .context("PTY reader の複製に失敗しました")?;
        let writer = pair
            .master
            .take_writer()
            .context("PTY writer の取得に失敗しました")?;

        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        std::thread::spawn(move || {
            let mut reader = reader;
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                    // 一時的な割り込み（SIGWINCH 等による EINTR）は EOF ではないので継続。
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            parser: vt100::Parser::new(rows, cols, 1000),
            master: pair.master,
            writer,
            child,
            rx,
            finished: false,
            rows,
            cols,
            goal_id,
            goal_title,
            cwd: cwd.display().to_string(),
            status_label,
            dod_items,
            dod_checks,
            assessing: false,
            child_count: None,
            comment_count: None,
            children: Vec::new(),
            action: None,
            status_changed_at: None,
            dod_changed_at: None,
            scrollback: 0,
            activity: Vec::new(),
            auto_record_attempted: false,
            input_state: CodexInputState::default(),
            dod,
        })
    }

    /// ログを delta 行スクロールする（正=過去へ、負=最新へ）。
    pub fn scroll_lines(&mut self, delta: isize) {
        let target = (self.scrollback as isize + delta).max(0) as usize;
        self.parser.screen_mut().set_scrollback(target);
        self.scrollback = self.parser.screen().scrollback();
    }

    /// 1 ページ分の行数（スクロール量）。
    pub fn page(&self) -> usize {
        self.rows.saturating_sub(1).max(1) as usize
    }

    /// 最古（バッファ先頭）までスクロールする。
    pub fn scroll_to_top(&mut self) {
        self.parser.screen_mut().set_scrollback(usize::MAX / 2);
        self.scrollback = self.parser.screen().scrollback();
    }

    /// 最新（ライブ）位置へ戻す。
    pub fn scroll_to_live(&mut self) {
        self.parser.screen_mut().set_scrollback(0);
        self.scrollback = 0;
    }

    /// codex の画面（直近の出力）を走査し、最後に実行された addness 操作を
    /// 「いま参照/書込中」ラベルとして self.action に反映する。update() から呼ぶ。
    fn refresh_action(&mut self) {
        let contents = self.parser.screen().contents();
        let lines: Vec<&str> = contents.lines().collect();
        // 画面下部（最近の出力）だけを見て、moved-on したら自然に消えるようにする。
        let start = lines.len().saturating_sub(10);
        let mut latest: Option<String> = None;
        for line in &lines[start..] {
            if let Some(idx) = line.find("addness ") {
                let rest = line[idx + "addness ".len()..].trim();
                latest = Some(action_label(rest));
            }
        }
        self.action = latest;
    }

    /// 子ゴールのライブリストを差し替える。新規 ID は一定時間ハイライトする。
    /// 初回（既存が空）の取得では全件を新着扱いしない。
    pub fn update_children(&mut self, incoming: Vec<(String, String, &'static str)>) {
        let had_any = !self.children.is_empty();
        let old_ids: std::collections::HashSet<String> =
            self.children.iter().map(|c| c.id.clone()).collect();
        let new_until = Instant::now() + std::time::Duration::from_secs(4);
        self.children = incoming
            .into_iter()
            .map(|(id, title, icon)| {
                let is_new = had_any && !old_ids.contains(&id);
                ChildGoal {
                    new_until: is_new.then_some(new_until),
                    id,
                    title,
                    icon,
                }
            })
            .collect();
    }

    /// Addness 側の更新ログへ 1 行追加する（古いものから捨てて最大 50 件保持）。
    pub fn push_activity(&mut self, line: String) {
        self.activity.push(line);
        let len = self.activity.len();
        if len > 50 {
            self.activity.drain(0..len - 50);
        }
    }

    /// PTY から届いた出力を取り込み、プロセス終了を検知する。毎フレーム呼ぶ。
    /// 画面に影響する変化（出力取り込み or 終了検知）があれば `true` を返す。
    pub fn update(&mut self) -> bool {
        let mut changed = false;
        while let Ok(bytes) = self.rx.try_recv() {
            self.parser.process(&bytes);
            changed = true;
        }
        // 画面が更新されたら「いま参照/書込中」インジケータを更新する。
        if changed {
            self.refresh_action();
        }
        if !self.finished && matches!(self.child.try_wait(), Ok(Some(_))) {
            self.finished = true;
            changed = true;
        }
        changed
    }

    /// 描画領域に合わせて PTY と vt100 のサイズを更新する（変化時のみ）。
    pub fn resize(&mut self, rows: u16, cols: u16) {
        let rows = rows.max(1);
        let cols = cols.max(1);
        if rows == self.rows && cols == self.cols {
            return;
        }
        self.rows = rows;
        self.cols = cols;
        self.parser.screen_mut().set_size(rows, cols);
        let _ = self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
    }

    /// 描画用の vt100 スクリーン。
    pub fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }

    /// キー入力を端末バイト列へ変換して PTY へ書き込む。
    pub fn input(&mut self, key: KeyEvent) {
        self.input_state.observe_key(key);
        let bytes = encode_key(key);
        if bytes.is_empty() {
            return;
        }
        let _ = self.writer.write_all(&bytes);
        let _ = self.writer.flush();
    }

    /// ユーザーが codex 内で `/exit` を送信し、その結果プロセスも終了しているか。
    pub fn should_close_after_exit_command(&self) -> bool {
        self.finished && self.input_state.exit_command_sent
    }

    /// ユーザーが codex に最後に送信した入力行。
    pub fn last_prompt(&self) -> Option<&str> {
        self.input_state.last_prompt.as_deref()
    }

    /// codex プロセスを終了させる（ペインを閉じる時に呼ぶ）。
    /// kill 後に wait してゾンビプロセス化を防ぐ。
    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    /// DoD を更新する。テキストが変わった場合のみ項目と判定をリセットする
    /// （ライブ更新で 3 秒ごとに呼ばれても、内容不変ならチェックを保持する）。
    /// DoD を更新する。内容が変わった場合のみ項目・判定を作り直し `true` を返す。
    pub fn set_dod(&mut self, dod: String) -> bool {
        if dod == self.dod {
            return false;
        }
        self.dod = dod;
        self.dod_items = split_dod_items(&self.dod);
        self.dod_checks = vec![None; self.dod_items.len()];
        true
    }

    /// DoD 自動判定の結果（項目インデックス → 達成可否）を反映する。
    pub fn apply_dod_results(&mut self, results: &[(usize, bool)]) {
        for &(i, met) in results {
            if let Some(slot) = self.dod_checks.get_mut(i) {
                *slot = Some(met);
            }
        }
    }
}

#[derive(Default)]
struct CodexInputState {
    line: String,
    exit_command_sent: bool,
    last_prompt: Option<String>,
}

impl CodexInputState {
    fn observe_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::ALT) {
            return;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) {
            if let KeyCode::Char('c' | 'C' | 'u' | 'U') = key.code {
                self.line.clear();
            }
            return;
        }

        match key.code {
            KeyCode::Char(c) => self.line.push(c),
            KeyCode::Backspace => {
                self.line.pop();
            }
            KeyCode::Enter => {
                let submitted = self.line.trim();
                if !submitted.is_empty() {
                    self.last_prompt = Some(submitted.to_string());
                }
                if submitted == "/exit" {
                    self.exit_command_sent = true;
                }
                self.line.clear();
            }
            KeyCode::Esc => {
                self.line.clear();
            }
            _ => {}
        }
    }
}

fn startup_recall_prompt() -> &'static str {
    "Addness TUIセッションを開始してください。起動時指示に従って対象ゴールを想起してから進めてください。"
}

fn eager_startup_recall_enabled() -> bool {
    std::env::var("ADDNESS_CODEX_EAGER_RECALL")
        .ok()
        .is_some_and(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "on"))
}

fn addness_tui_developer_instructions() -> &'static str {
    r#"Addness TUIから起動されました。

起動直後は Addness CLI を実行せず、ユーザーの最初の入力を待ってください。
最初の依頼に軽く応答できる場合は、TUIから渡された軽量コンテキストだけを使って即応して構いません。

軽量コンテキスト:
- ADDNESS_GOAL_ID: 対象ゴールID
- ADDNESS_GOAL_TITLE: 対象ゴール名
- ADDNESS_GOAL_STATUS: 対象ゴールの現在状態
- ADDNESS_GOAL_DOD: 対象ゴールのDoD/完了基準

実作業の判断、body/コメント/成果物/子ゴールの確認、引き継ぎ、サブエージェント分解が必要になったら、
作業に入る前に Addness CLI を使って対象ゴールを想起してください。

必要時に実行するコマンド:
`"$ADDNESS_BIN" goal get "$ADDNESS_GOAL_ID" --json --with-deliverable --with-comment`

Addness はこの組織/プロジェクト専用の作業DB・引き継ぎ点として扱ってください。

読み取り時:
1. body、DoD(description/definitionOfDone)、コメント、成果物、子ゴール、作業フォルダ/ブランチを確認する。
2. DoDが空・曖昧・現在の作業に対して不足していないかを見る。
3. 子ゴールが、実際に分担・並列化・サブエージェント化できる作業単位に分かれているかを見る。

書き込み時:
- 起動しただけでは Addness に書き込まない。
- Addnessに書き込むのは、サブエージェント/分担/並列作業/別セッションへの引き継ぎが必要になった時、またはユーザーが明示的に記録を求めた時を基本にする。
- サブエージェントが必要な場合は、必要な作業単位を子ゴールとして作成または更新する。子ゴールのtitleは作業名、descriptionはそのサブエージェントのDoD、bodyは入力情報・作業フォルダ・ブランチ・期待成果物・現在地を書く。
- 親ゴールのbodyは、複数エージェントが迷わないための共有状態・担当分解・次の合流点が必要な時に更新する。
- DoDが不十分でも、単独でそのまま進められるなら勝手に書き換えず、短く不足を指摘して必要なら質問する。サブエージェントに渡すために契約が必要な場合だけ `"$ADDNESS_BIN" goal update "$ADDNESS_GOAL_ID" --description "..." --json` で具体化する。
- コメントは構造化フィールドに置けない質問や補足だけに使う。

Addnessを読んだ場合は、何を読んだか、Addnessへの書き込みが必要か、次に進めることを短く共有してください。"#
}

enum CodexConfigKey {
    DeveloperInstructions,
}

impl CodexConfigKey {
    fn as_str(&self) -> &'static str {
        match self {
            CodexConfigKey::DeveloperInstructions => "developer_instructions",
        }
    }
}

fn codex_config_arg(key: CodexConfigKey, value: &str) -> String {
    format!("{}={}", key.as_str(), toml_basic_string(value))
}

fn toml_basic_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// DoD 自動判定で codex に強制する出力 JSON Schema。
/// `{ "results": [ { "index": <int>, "met": <bool> } ] }` の形を要求する。
pub fn dod_assessment_schema() -> String {
    r#"{
  "type": "object",
  "additionalProperties": false,
  "required": ["results"],
  "properties": {
    "results": {
      "type": "array",
      "items": {
        "type": "object",
        "additionalProperties": false,
        "required": ["index", "met"],
        "properties": {
          "index": { "type": "integer", "minimum": 0 },
          "met": { "type": "boolean" }
        }
      }
    }
  }
}"#
    .to_string()
}

/// DoD 自動判定用のプロンプトを組み立てる。各項目に番号を振って提示する。
pub fn build_dod_assessment_prompt(items: &[String]) -> String {
    let mut listed = String::new();
    for (i, item) in items.iter().enumerate() {
        listed.push_str(&format!("{i}: {item}\n"));
    }
    format!(
        r#"あなたはコードレビュー担当です。コードの変更は一切行わないでください（read-only）。

現在のリポジトリの作業ツリーの状態（`git diff HEAD` や関連ファイルの内容）を調べ、
以下の各「完了基準(DoD)項目」が**現時点で満たされているか**を判定してください。

DoD項目（番号: 内容）:
{listed}
判定結果は、指定された JSON Schema に厳密に従って出力してください。
各項目について index（番号）と met（満たされていれば true、そうでなければ false）を返します。
確証が持てない項目は met=false としてください。"#
    )
}

/// crossterm の `KeyEvent` を xterm 互換の端末バイト列へエンコードする。
fn encode_key(key: KeyEvent) -> Vec<u8> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);

    let mut out: Vec<u8> = match key.code {
        KeyCode::Char(c) => {
            if ctrl {
                let upper = c.to_ascii_uppercase() as u32;
                if (b'@' as u32..=b'_' as u32).contains(&upper) {
                    vec![(upper as u8) & 0x1f]
                } else if c == ' ' {
                    vec![0]
                } else {
                    let mut buf = [0u8; 4];
                    c.encode_utf8(&mut buf).as_bytes().to_vec()
                }
            } else {
                let mut buf = [0u8; 4];
                c.encode_utf8(&mut buf).as_bytes().to_vec()
            }
        }
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Esc => vec![0x1b],
        KeyCode::Left => b"\x1b[D".to_vec(),
        KeyCode::Right => b"\x1b[C".to_vec(),
        KeyCode::Up => b"\x1b[A".to_vec(),
        KeyCode::Down => b"\x1b[B".to_vec(),
        KeyCode::Home => b"\x1b[H".to_vec(),
        KeyCode::End => b"\x1b[F".to_vec(),
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        KeyCode::Insert => b"\x1b[2~".to_vec(),
        KeyCode::F(n) => match n {
            1 => b"\x1bOP".to_vec(),
            2 => b"\x1bOQ".to_vec(),
            3 => b"\x1bOR".to_vec(),
            4 => b"\x1bOS".to_vec(),
            5 => b"\x1b[15~".to_vec(),
            6 => b"\x1b[17~".to_vec(),
            7 => b"\x1b[18~".to_vec(),
            8 => b"\x1b[19~".to_vec(),
            9 => b"\x1b[20~".to_vec(),
            10 => b"\x1b[21~".to_vec(),
            11 => b"\x1b[23~".to_vec(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    };

    // Alt 修飾は ESC プレフィックスで表現する（Esc 単体は除く）。
    if alt && !out.is_empty() && key.code != KeyCode::Esc {
        let mut prefixed = vec![0x1b];
        prefixed.append(&mut out);
        return prefixed;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn key_mod(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    #[test]
    fn encode_plain_char() {
        assert_eq!(encode_key(key(KeyCode::Char('a'))), vec![b'a']);
    }

    #[test]
    fn encode_ctrl_c_is_etx() {
        assert_eq!(
            encode_key(key_mod(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            vec![0x03]
        );
    }

    #[test]
    fn encode_ctrl_space_is_nul() {
        assert_eq!(
            encode_key(key_mod(KeyCode::Char(' '), KeyModifiers::CONTROL)),
            vec![0]
        );
    }

    #[test]
    fn encode_special_keys() {
        assert_eq!(encode_key(key(KeyCode::Enter)), vec![b'\r']);
        assert_eq!(encode_key(key(KeyCode::Esc)), vec![0x1b]);
        assert_eq!(encode_key(key(KeyCode::Backspace)), vec![0x7f]);
        assert_eq!(encode_key(key(KeyCode::Up)), b"\x1b[A".to_vec());
        assert_eq!(encode_key(key(KeyCode::Left)), b"\x1b[D".to_vec());
    }

    #[test]
    fn encode_alt_prefixes_escape() {
        assert_eq!(
            encode_key(key_mod(KeyCode::Char('x'), KeyModifiers::ALT)),
            vec![0x1b, b'x']
        );
    }

    #[test]
    fn codex_input_state_detects_exit_command_on_enter() {
        let mut state = CodexInputState::default();
        for ch in "/exit".chars() {
            state.observe_key(key(KeyCode::Char(ch)));
        }
        assert!(!state.exit_command_sent);

        state.observe_key(key(KeyCode::Enter));

        assert!(state.exit_command_sent);
    }

    #[test]
    fn codex_input_state_records_last_prompt_on_enter() {
        let mut state = CodexInputState::default();
        for ch in "  cargo test を実行して  ".chars() {
            state.observe_key(key(KeyCode::Char(ch)));
        }
        state.observe_key(key(KeyCode::Enter));

        assert_eq!(state.last_prompt.as_deref(), Some("cargo test を実行して"));
    }

    #[test]
    fn codex_input_state_keeps_last_prompt_on_blank_enter() {
        let mut state = CodexInputState::default();
        for ch in "最初の依頼".chars() {
            state.observe_key(key(KeyCode::Char(ch)));
        }
        state.observe_key(key(KeyCode::Enter));
        state.observe_key(key(KeyCode::Enter));

        assert_eq!(state.last_prompt.as_deref(), Some("最初の依頼"));
    }

    #[test]
    fn codex_input_state_ignores_non_exit_lines() {
        let mut state = CodexInputState::default();
        for ch in "please /exit later".chars() {
            state.observe_key(key(KeyCode::Char(ch)));
        }
        state.observe_key(key(KeyCode::Enter));

        assert!(!state.exit_command_sent);
    }

    #[test]
    fn codex_input_state_handles_backspace_before_exit() {
        let mut state = CodexInputState::default();
        for ch in "/exitt".chars() {
            state.observe_key(key(KeyCode::Char(ch)));
        }
        state.observe_key(key(KeyCode::Backspace));
        state.observe_key(key(KeyCode::Enter));

        assert!(state.exit_command_sent);
    }

    #[test]
    fn codex_input_state_clears_line_on_ctrl_u() {
        let mut state = CodexInputState::default();
        for ch in "/exit".chars() {
            state.observe_key(key(KeyCode::Char(ch)));
        }
        state.observe_key(key_mod(KeyCode::Char('u'), KeyModifiers::CONTROL));
        state.observe_key(key(KeyCode::Enter));

        assert!(!state.exit_command_sent);
    }

    #[test]
    fn split_dod_items_trims_and_skips_blank_lines() {
        let dod = "  /authをmodule化\n\n テストが緑 \n";
        assert_eq!(
            split_dod_items(dod),
            vec!["/authをmodule化".to_string(), "テストが緑".to_string()]
        );
    }

    #[test]
    fn split_dod_items_empty() {
        assert!(split_dod_items("   \n\n").is_empty());
    }

    #[test]
    fn startup_instructions_defer_full_addness_recall_until_needed() {
        let prompt = startup_recall_prompt();
        let instructions = addness_tui_developer_instructions();

        assert!(prompt.contains("Addness TUIセッションを開始"));
        assert!(prompt.len() < 80 * 2);
        assert!(instructions.contains("起動直後は Addness CLI を実行せず"));
        assert!(instructions.contains("ユーザーの最初の入力を待ってください"));
        assert!(instructions.contains("軽量コンテキスト"));
        assert!(instructions.contains("ADDNESS_GOAL_TITLE"));
        assert!(instructions.contains("ADDNESS_GOAL_STATUS"));
        assert!(instructions.contains("ADDNESS_GOAL_DOD"));
        assert!(instructions.contains("必要時に実行するコマンド"));
        assert!(instructions.contains("\"$ADDNESS_BIN\" goal get \"$ADDNESS_GOAL_ID\""));
        assert!(instructions.contains("--json --with-deliverable --with-comment"));
        assert!(instructions.contains("組織/プロジェクト専用の作業DB"));
        assert!(instructions.contains("起動しただけでは Addness に書き込まない"));
        assert!(instructions.contains("サブエージェント/分担/並列作業/別セッションへの引き継ぎ"));
        assert!(instructions.contains("goal update \"$ADDNESS_GOAL_ID\" --description"));
    }

    #[test]
    fn codex_config_arg_escapes_developer_instructions_as_toml() {
        let arg = codex_config_arg(
            CodexConfigKey::DeveloperInstructions,
            "quote: \" / path: C:\\tmp\nnext\tline",
        );

        assert_eq!(
            arg,
            "developer_instructions=\"quote: \\\" / path: C:\\\\tmp\\nnext\\tline\""
        );
    }
}
