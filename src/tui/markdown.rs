//! AI アシスタントメッセージを Markdown として整形し、ratatui の
//! スタイル付き行（`Vec<Span>`）へ変換するモジュール。
//!
//! 既存のログ描画（`ui.rs` の `RenderedCodexLine`）はプレフィックス列と
//! 幅折り返しを自前で管理しているため、ここでは「本文スパンを指定幅で
//! 折り返した視覚行」の列を返すだけに徹する。プレフィックス（`返答 | ` 等）は
//! 呼び出し側で各行に付与する。
//!
//! パーサには軽量な `pulldown-cmark` を用い、テーマ配色への割り当てと
//! 幅折り返しのみ自前で行う（シンタックスハイライトは導入しない）。

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::Style;
use ratatui::text::Span;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Markdown 要素ごとの配色。呼び出し側（`ui.rs`）がテーマ定数から構築する。
#[derive(Debug, Clone, Copy)]
pub struct MarkdownStyles {
    /// 通常本文。
    pub text: Style,
    /// 見出し（太字＋色分け）。
    pub heading: Style,
    /// 太字。
    pub strong: Style,
    /// 斜体。
    pub emphasis: Style,
    /// インラインコード（背景色付き）。
    pub inline_code: Style,
    /// コードブロック本文。
    pub code_block: Style,
    /// コードブロックの言語ラベル。
    pub code_label: Style,
    /// 引用本文。
    pub quote: Style,
    /// 引用の縦線マーカー。
    pub quote_marker: Style,
    /// リストの記号（`•` / `1.`）。
    pub list_marker: Style,
    /// 罫線・水平線。
    pub rule: Style,
    /// リンク（本文はそのまま、装飾のみ付与）。
    pub link: Style,
}

/// アシスタントメッセージを Markdown 整形し、`width` 幅で折り返した
/// 視覚行の本文スパン列を返す。各要素が 1 視覚行に対応する。
///
/// 返り値は必ず 1 要素以上（空メッセージでも空行 1 つ）。
pub fn render_assistant_markdown(
    text: &str,
    width: usize,
    styles: &MarkdownStyles,
) -> Vec<Vec<Span<'static>>> {
    let width = width.max(1);
    let mut renderer = Renderer::new(width, *styles);
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    for event in Parser::new_ext(text, options) {
        renderer.handle(event);
    }
    renderer.finish()
}

/// 描画途中で積み上げる文字＋スタイルの組。折り返し時に再スパン化する。
type StyledChar = (char, Style);

struct Renderer {
    width: usize,
    styles: MarkdownStyles,
    /// 確定した視覚行（本文スパンのみ）。
    out: Vec<Vec<Span<'static>>>,
    /// 現在のインラインバッファ。
    cur: Vec<StyledChar>,
    /// スタイルスタック（現在値は末尾）。
    style_stack: Vec<Style>,
    /// 入れ子（リスト項目・引用）の継続プレフィックス。
    base_first: Vec<Span<'static>>,
    base_cont: Vec<Span<'static>>,
    /// 直近で開いたリスト項目のマーカー（最初のフラッシュで消費）。
    pending_marker: Option<(Vec<Span<'static>>, Vec<Span<'static>>)>,
    /// プレフィックス退避スタック（項目・引用の入れ子復帰用）。
    prefix_stack: Vec<(Vec<Span<'static>>, Vec<Span<'static>>)>,
    /// 順序付きリストの次番号スタック。
    ordered_stack: Vec<Option<u64>>,
    /// 入れ子の深さ（トップレベル空行判定用）。
    depth: usize,
    /// コードブロック収集中か。
    code_block: Option<CodeBlockState>,
    /// テーブルの行バッファ。
    table_row: Option<Vec<String>>,
    in_table: bool,
}

struct CodeBlockState {
    lang: String,
    buffer: String,
}

impl Renderer {
    fn new(width: usize, styles: MarkdownStyles) -> Self {
        Self {
            width,
            styles,
            out: Vec::new(),
            cur: Vec::new(),
            style_stack: Vec::new(),
            base_first: Vec::new(),
            base_cont: Vec::new(),
            pending_marker: None,
            prefix_stack: Vec::new(),
            ordered_stack: Vec::new(),
            depth: 0,
            code_block: None,
            table_row: None,
            in_table: false,
        }
    }

    fn cur_style(&self) -> Style {
        self.style_stack.last().copied().unwrap_or(self.styles.text)
    }

    fn handle(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.on_text(&text),
            Event::Code(code) => self.push_str(&code, self.styles.inline_code),
            Event::SoftBreak | Event::HardBreak => self.push_char(' ', self.cur_style()),
            Event::Rule => self.emit_rule(),
            Event::TaskListMarker(done) => {
                let mark = if done { "[x] " } else { "[ ] " };
                self.push_str(mark, self.styles.text);
            }
            // HTML やその他イベントは本文としては無視する。
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => self.block_break(),
            Tag::Heading { level, .. } => {
                self.block_break();
                self.style_stack.push(self.heading_style(level));
            }
            Tag::BlockQuote(_) => {
                self.block_break();
                self.depth += 1;
                self.prefix_stack
                    .push((self.base_first.clone(), self.base_cont.clone()));
                let marker = Span::styled("\u{258f} ", self.styles.quote_marker);
                self.base_first = extend_prefix(&self.base_cont, marker.clone());
                self.base_cont = extend_prefix(&self.base_cont, marker);
                self.style_stack.push(self.styles.quote);
            }
            Tag::CodeBlock(kind) => {
                self.block_break();
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split_whitespace().next().unwrap_or("").to_string()
                    }
                    CodeBlockKind::Indented => String::new(),
                };
                self.code_block = Some(CodeBlockState {
                    lang,
                    buffer: String::new(),
                });
            }
            Tag::List(start) => {
                self.block_break();
                self.ordered_stack.push(start);
            }
            Tag::Item => {
                self.depth += 1;
                self.prefix_stack
                    .push((self.base_first.clone(), self.base_cont.clone()));
                let marker_text = match self.ordered_stack.last_mut() {
                    Some(Some(n)) => {
                        let text = format!("{n}. ");
                        *n += 1;
                        text
                    }
                    _ => "\u{2022} ".to_string(),
                };
                let marker_width = UnicodeWidthStr::width(marker_text.as_str());
                let first = extend_prefix(
                    &self.base_cont,
                    Span::styled(marker_text, self.styles.list_marker),
                );
                let cont = extend_prefix(
                    &self.base_cont,
                    Span::styled(" ".repeat(marker_width), self.styles.text),
                );
                self.pending_marker = Some((first.clone(), cont.clone()));
                self.base_first = first;
                self.base_cont = cont;
            }
            Tag::Emphasis => self.style_stack.push(self.emphasis_style()),
            Tag::Strong => self.style_stack.push(self.strong_style()),
            Tag::Strikethrough => self.style_stack.push(self.cur_style()),
            Tag::Link { .. } => self.style_stack.push(self.styles.link),
            Tag::Table(_) => {
                self.block_break();
                self.in_table = true;
            }
            Tag::TableRow | Tag::TableHead => self.table_row = Some(Vec::new()),
            Tag::TableCell => self.cur.clear(),
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => self.flush_line(),
            TagEnd::Heading(_) => {
                self.flush_line();
                self.style_stack.pop();
            }
            TagEnd::BlockQuote(_) => {
                self.style_stack.pop();
                self.pop_prefix();
                self.depth = self.depth.saturating_sub(1);
            }
            TagEnd::CodeBlock => {
                if let Some(state) = self.code_block.take() {
                    self.emit_code_block(state);
                }
            }
            TagEnd::List(_) => {
                self.ordered_stack.pop();
            }
            TagEnd::Item => {
                // タイトリスト（段落なし）の項目本文を取りこぼさない。
                if !self.cur.is_empty() || self.pending_marker.is_some() {
                    self.flush_line();
                }
                self.pop_prefix();
                self.depth = self.depth.saturating_sub(1);
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link => {
                self.style_stack.pop();
            }
            TagEnd::Table => {
                self.in_table = false;
            }
            TagEnd::TableRow | TagEnd::TableHead => {
                if let Some(cells) = self.table_row.take() {
                    let text = cells.join("  ");
                    self.push_str(&text, self.styles.text);
                    self.flush_line();
                }
            }
            TagEnd::TableCell => {
                if let Some(row) = self.table_row.as_mut() {
                    let cell: String = self.cur.iter().map(|(c, _)| *c).collect();
                    row.push(cell.trim().to_string());
                }
                self.cur.clear();
            }
            _ => {}
        }
    }

    fn on_text(&mut self, text: &str) {
        if let Some(state) = self.code_block.as_mut() {
            state.buffer.push_str(text);
            return;
        }
        let style = self.cur_style();
        self.push_str(text, style);
    }

    fn push_str(&mut self, text: &str, style: Style) {
        for ch in text.chars() {
            self.push_char(ch, style);
        }
    }

    fn push_char(&mut self, ch: char, style: Style) {
        if ch == '\r' {
            return;
        }
        self.cur.push((ch, style));
    }

    fn heading_style(&self, level: HeadingLevel) -> Style {
        // レベルによらず統一配色（太字＋色分け）。深い見出しは modifier を弱める。
        let _ = level;
        self.styles.heading
    }

    fn strong_style(&self) -> Style {
        merge(self.cur_style(), self.styles.strong)
    }

    fn emphasis_style(&self) -> Style {
        merge(self.cur_style(), self.styles.emphasis)
    }

    /// トップレベルのブロック境界で空行を 1 つ挿入する。
    fn block_break(&mut self) {
        if self.depth == 0 && !self.out.is_empty() {
            self.out.push(Vec::new());
        }
    }

    /// 現在のインラインバッファを 1 ブロックとして折り返し確定する。
    fn flush_line(&mut self) {
        let content = std::mem::take(&mut self.cur);
        let (first, cont) = match self.pending_marker.take() {
            Some(pair) => pair,
            None => (self.base_first.clone(), self.base_cont.clone()),
        };
        self.emit_block(&first, &cont, &content);
    }

    fn emit_block(
        &mut self,
        prefix_first: &[Span<'static>],
        prefix_cont: &[Span<'static>],
        content: &[StyledChar],
    ) {
        let prefix_width = prefix_display_width(prefix_first);
        let avail = self.width.saturating_sub(prefix_width).max(1);
        let wrapped = wrap_styled(content, avail);
        for (idx, line_spans) in wrapped.into_iter().enumerate() {
            let mut spans = if idx == 0 {
                prefix_first.to_vec()
            } else {
                prefix_cont.to_vec()
            };
            spans.extend(line_spans);
            self.out.push(spans);
        }
    }

    fn emit_code_block(&mut self, state: CodeBlockState) {
        let first = self.base_first.clone();
        let cont = self.base_cont.clone();
        if !state.lang.is_empty() {
            let label = vec![Span::styled(
                format!("```{}", state.lang),
                self.styles.code_label,
            )];
            self.emit_prefixed(&first, &label);
        }
        let body = state.buffer.trim_end_matches('\n');
        for raw in body.split('\n') {
            let chars: Vec<StyledChar> = raw.chars().map(|c| (c, self.styles.code_block)).collect();
            self.emit_block(&cont, &cont, &chars);
        }
    }

    fn emit_rule(&mut self) {
        self.block_break();
        let avail = self
            .width
            .saturating_sub(prefix_display_width(&self.base_cont));
        let bar = "\u{2500}".repeat(avail.max(1));
        let content: Vec<StyledChar> = bar.chars().map(|c| (c, self.styles.rule)).collect();
        let first = self.base_first.clone();
        self.emit_block(&first, &first, &content);
    }

    /// プレフィックス＋固定スパン列をそのまま 1 行として追加する。
    fn emit_prefixed(&mut self, prefix: &[Span<'static>], spans: &[Span<'static>]) {
        let mut line = prefix.to_vec();
        line.extend(spans.iter().cloned());
        self.out.push(line);
    }

    fn pop_prefix(&mut self) {
        if let Some((first, cont)) = self.prefix_stack.pop() {
            self.base_first = first;
            self.base_cont = cont;
        }
        self.pending_marker = None;
    }

    fn finish(mut self) -> Vec<Vec<Span<'static>>> {
        if !self.cur.is_empty() {
            self.flush_line();
        }
        if self.out.is_empty() {
            self.out.push(Vec::new());
        }
        self.out
    }
}

/// 既存スタイルへ別スタイルの前景色・modifier を重ねる。
fn merge(base: Style, overlay: Style) -> Style {
    let mut merged = base;
    if let Some(fg) = overlay.fg {
        merged = merged.fg(fg);
    }
    if let Some(bg) = overlay.bg {
        merged = merged.bg(bg);
    }
    merged.add_modifier(overlay.add_modifier)
}

fn extend_prefix(base: &[Span<'static>], extra: Span<'static>) -> Vec<Span<'static>> {
    let mut prefix = base.to_vec();
    prefix.push(extra);
    prefix
}

fn prefix_display_width(prefix: &[Span<'static>]) -> usize {
    prefix
        .iter()
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
        .sum()
}

/// 文字＋スタイル列を `width` 幅（表示幅・文字単位）で折り返し、
/// 連続する同一スタイルをまとめてスパン化する。
fn wrap_styled(chars: &[StyledChar], width: usize) -> Vec<Vec<Span<'static>>> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut line: Vec<StyledChar> = Vec::new();
    let mut line_width = 0usize;
    for &(ch, style) in chars {
        if ch == '\n' {
            lines.push(coalesce(&line));
            line.clear();
            line_width = 0;
            continue;
        }
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if line_width > 0 && line_width + ch_width > width {
            lines.push(coalesce(&line));
            line.clear();
            line_width = 0;
        }
        line.push((ch, style));
        line_width += ch_width;
    }
    lines.push(coalesce(&line));
    lines
}

/// 連続する同一スタイルの文字をひとつの `Span` にまとめる。
fn coalesce(chars: &[StyledChar]) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut buffer = String::new();
    let mut current: Option<Style> = None;
    for &(ch, style) in chars {
        match current {
            Some(prev) if prev == style => buffer.push(ch),
            _ => {
                if let Some(prev) = current {
                    spans.push(Span::styled(std::mem::take(&mut buffer), prev));
                }
                buffer.push(ch);
                current = Some(style);
            }
        }
    }
    if let Some(style) = current {
        spans.push(Span::styled(buffer, style));
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::{Color, Modifier};

    fn styles() -> MarkdownStyles {
        MarkdownStyles {
            text: Style::default().fg(Color::White),
            heading: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            strong: Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
            emphasis: Style::default().add_modifier(Modifier::ITALIC),
            inline_code: Style::default().fg(Color::Yellow).bg(Color::Black),
            code_block: Style::default().fg(Color::Gray).bg(Color::Black),
            code_label: Style::default().fg(Color::DarkGray),
            quote: Style::default()
                .fg(Color::Gray)
                .add_modifier(Modifier::ITALIC),
            quote_marker: Style::default().fg(Color::DarkGray),
            list_marker: Style::default().fg(Color::Cyan),
            rule: Style::default().fg(Color::DarkGray),
            link: Style::default().add_modifier(Modifier::UNDERLINED),
        }
    }

    fn line_text(spans: &[Span<'static>]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn heading_is_bold_colored() {
        let lines = render_assistant_markdown("# 見出し", 40, &styles());
        assert_eq!(lines.len(), 1);
        assert_eq!(line_text(&lines[0]), "見出し");
        let span = &lines[0][0];
        assert!(span.style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(span.style.fg, Some(Color::Cyan));
    }

    #[test]
    fn inline_styles_are_applied() {
        let lines = render_assistant_markdown("これは **太字** と `コード`", 40, &styles());
        assert_eq!(lines.len(), 1);
        let text = line_text(&lines[0]);
        assert!(text.contains("太字"));
        assert!(text.contains("コード"));
        let bold = lines[0]
            .iter()
            .find(|s| s.content.contains("太字"))
            .unwrap();
        assert!(bold.style.add_modifier.contains(Modifier::BOLD));
        let code = lines[0]
            .iter()
            .find(|s| s.content.contains("コード"))
            .unwrap();
        assert_eq!(code.style.bg, Some(Color::Black));
    }

    #[test]
    fn code_block_lines_use_code_style_and_label() {
        let md = "```rust\nfn main() {}\nlet x = 1;\n```";
        let lines = render_assistant_markdown(md, 40, &styles());
        // 言語ラベル + 2 行のコード。
        assert_eq!(lines.len(), 3);
        assert_eq!(line_text(&lines[0]), "```rust");
        assert_eq!(line_text(&lines[1]), "fn main() {}");
        assert_eq!(line_text(&lines[2]), "let x = 1;");
        let code_span = &lines[1][0];
        assert_eq!(code_span.style.bg, Some(Color::Black));
    }

    #[test]
    fn bullet_list_has_markers() {
        let md = "- 一つ目\n- 二つ目";
        let lines = render_assistant_markdown(md, 40, &styles());
        assert_eq!(lines.len(), 2);
        assert_eq!(line_text(&lines[0]), "\u{2022} 一つ目");
        assert_eq!(line_text(&lines[1]), "\u{2022} 二つ目");
        assert_eq!(lines[0][0].style.fg, Some(Color::Cyan));
    }

    #[test]
    fn ordered_list_numbers_increment() {
        let md = "1. 最初\n2. 次";
        let lines = render_assistant_markdown(md, 40, &styles());
        assert_eq!(line_text(&lines[0]), "1. 最初");
        assert_eq!(line_text(&lines[1]), "2. 次");
    }

    #[test]
    fn blockquote_has_vertical_bar() {
        let lines = render_assistant_markdown("> 引用文", 40, &styles());
        assert_eq!(lines.len(), 1);
        assert_eq!(line_text(&lines[0]), "\u{258f} 引用文");
        let body = lines[0]
            .iter()
            .find(|s| s.content.contains("引用文"))
            .unwrap();
        assert!(body.style.add_modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn long_paragraph_wraps_to_width() {
        let md = "abcdefghij klmnopqrst";
        let lines = render_assistant_markdown(md, 10, &styles());
        assert!(lines.len() >= 2);
        for line in &lines {
            let width: usize = line
                .iter()
                .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
                .sum();
            assert!(width <= 10, "line too wide: {width}");
        }
    }

    #[test]
    fn empty_input_yields_single_blank_line() {
        let lines = render_assistant_markdown("", 40, &styles());
        assert_eq!(lines.len(), 1);
        assert!(lines[0].is_empty());
    }

    #[test]
    fn paragraphs_separated_by_blank_line() {
        let lines = render_assistant_markdown("段落一\n\n段落二", 40, &styles());
        assert_eq!(lines.len(), 3);
        assert_eq!(line_text(&lines[0]), "段落一");
        assert!(lines[1].is_empty());
        assert_eq!(line_text(&lines[2]), "段落二");
    }
}
