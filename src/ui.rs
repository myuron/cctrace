//! Ratatui による描画 (SPEC §7 / §8)。
//!
//! タイムライン表のブロック生成 (`build_blocks`) は描画から独立した純関数にし、
//! 「Skill 起動」「attributionSkill 由来」のハイライトや折りたたみ判定を
//! 単体テストできるようにする。

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, List, ListItem, ListState, Paragraph, Row, Table};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::{App, Screen};
use crate::model::{Block as ContentBlock, Entry, EntryKind, HookSummary};

const SKILL_COLOR: Color = Color::Magenta;
/// フック起動を目立たせる色。Skill(Magenta)/tool(Yellow) 等と混ざらない色を選ぶ。
const HOOK_COLOR: Color = Color::LightBlue;

/// タイムライン表の Time 列幅 (`draw_timeline` の Constraint と一致させる)。
const TIME_COL_WIDTH: u16 = 8;
/// タイムライン表の Kind 列幅 (同上)。
const KIND_COL_WIDTH: u16 = 10;
/// Detail 列以外がテーブル内で消費する幅。選択記号 (▶ ＝2)、列間スペース×2、
/// Time/Kind 列の合計。Detail 列の折り返し幅を求めるために描画側と共有する。
const NON_DETAIL_WIDTH: u16 = 2 + 2 + TIME_COL_WIDTH + KIND_COL_WIDTH;

/// 現在の画面に応じて全体を描画する。スクロール上限は viewport 依存のためここで確定する。
pub fn draw(frame: &mut Frame, app: &mut App) {
    match app.screen {
        Screen::SessionList => draw_session_list(frame, app),
        Screen::Timeline => draw_timeline(frame, app),
    }
}

fn draw_session_list(frame: &mut Frame, app: &App) {
    let areas = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(frame.area());

    frame.render_widget(
        Line::from(Span::styled(
            " cctrace — sessions ",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        areas[0],
    );

    if app.sessions.is_empty() {
        let empty =
            Paragraph::new("このプロジェクトのセッションが見つかりません (~/.claude/projects)。")
                .block(Block::default().borders(Borders::ALL));
        frame.render_widget(empty, areas[1]);
    } else {
        let items: Vec<ListItem> = app
            .sessions
            .iter()
            .map(|s| {
                let title = s.title.as_deref().unwrap_or("(no title)");
                ListItem::new(format!("{}  {}", format_mtime(s.modified), title))
            })
            .collect();
        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL))
            .highlight_style(
                Style::default()
                    .bg(Color::Blue)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ");
        let mut state = ListState::default();
        state.select(Some(app.list_selected));
        frame.render_stateful_widget(list, areas[1], &mut state);
    }

    frame.render_widget(
        hint_line("↑/↓ or j/k: 移動   Enter: 開く   q: 終了"),
        areas[2],
    );
}

fn draw_timeline(frame: &mut Frame, app: &mut App) {
    let areas = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(frame.area());

    let Some(open) = &mut app.open else {
        return;
    };

    // フック起動数はツールの核 (フックが起動したかの確認)。ヘッダに常時出す。
    let hook_count = open.blocks.iter().filter(|b| b.is_hook()).count();
    let header = format!(
        " {}   entries: {}{}   hooks: {}   branches: {}",
        open.meta.id,
        open.data.entries.len(),
        skipped_suffix(open.data.skipped_lines),
        hook_count,
        if app.show_branches { "on" } else { "off" },
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            header,
            Style::default().add_modifier(Modifier::BOLD),
        ))),
        areas[0],
    );

    // ブロックはキャッシュ済み (OpenSession)。展開状態に応じて可視行へ平坦化する。
    // 再描画はキー入力駆動 (main のイベントループ) なので、入力ごとに 1 度だけ
    // 平坦化するコストは無視できる。選択ブロックの先頭行の位置も併せて求める。
    // Detail 列の実効幅。枠線 (2) と選択記号・列間・Time/Kind 列 (NON_DETAIL_WIDTH) を
    // 差し引いた残り。ここへ折り返してクリップされない全文表示にする。
    let detail_width = areas[1].width.saturating_sub(2 + NON_DETAIL_WIDTH) as usize;

    let mut rows: Vec<Row> = Vec::new();
    let mut selected_row = 0usize;
    for (i, block) in open.blocks.iter().enumerate() {
        if i == open.selected {
            selected_row = rows.len();
        }
        let expanded = open.expanded.get(i).copied().unwrap_or(false);
        rows.extend(block_rows(block, expanded, detail_width));
    }

    let widths = [
        Constraint::Length(TIME_COL_WIDTH),
        Constraint::Length(KIND_COL_WIDTH),
        Constraint::Min(10),
    ];
    let header = Row::new(["Time", "Kind", "Detail"]).style(
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    );
    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL))
        .row_highlight_style(
            Style::default()
                .bg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    // 選択ブロックの先頭行を選択状態にし、可視領域維持のスクロールは TableState に委ねる。
    let selection = (!open.blocks.is_empty()).then_some(selected_row);
    open.table_state.select(selection);
    frame.render_stateful_widget(table, areas[1], &mut open.table_state);

    frame.render_widget(
        hint_line(
            "↑/↓ or j/k: 選択   Enter: 開閉   g/G: 先頭/末尾   h/H: フック   b: 分岐表示   Esc: 一覧   q: 終了",
        ),
        areas[2],
    );
}

/// タイムライン表の 1 ブロック。user/assistant の 1 content ブロック、または
/// system/attachment/unknown の 1 エントリに対応する。描画から独立した純データにして
/// ハイライトや折りたたみ判定を単体テストできるようにする。
///
/// 同一エントリの 2 ブロック目以降は time/kind を空にして表を詰め、kind 列が
/// 埋まっている行が新しいエントリの先頭であることを示す。折りたたみ可否は Detail 幅への
/// 折り返し後の表示行数で決まる (`block_rows`) ため、ここでは幅非依存の論理行のみ持つ。
#[derive(Debug, Clone)]
pub struct TimelineBlock {
    time: String,
    kind: Line<'static>,
    lines: Vec<Line<'static>>,
    /// フック起動ブロックか。ヘッダのフック数集計と `h`/`H` ジャンプに使う。
    is_hook: bool,
}

impl TimelineBlock {
    /// フック起動を表すブロックか (発見性向上のジャンプ・集計に用いる)。
    pub fn is_hook(&self) -> bool {
        self.is_hook
    }
}

/// エントリ列をタイムライン表のブロックへ変換する (SPEC §6.2 / §7)。
///
/// timestamp 昇順で並べる (ISO8601 は辞書順=時系列順)。安定ソートなので同一
/// timestamp や timestamp 欠落エントリは記録順を保つ。
/// 既定は主系列のみを線形表示。`show_branches` が真のときサイドチェーンも含める。
/// メタ情報 (`EntryKind::Meta`) は常に非表示。
pub fn build_blocks(entries: &[Entry], show_branches: bool) -> Vec<TimelineBlock> {
    let mut ordered: Vec<&Entry> = entries.iter().collect();
    ordered.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

    let mut blocks = Vec::new();
    for entry in ordered {
        if !entry.kind.is_visible_by_default() {
            continue;
        }
        if entry.is_sidechain && !show_branches {
            continue;
        }
        entry_blocks(entry, &mut blocks);
    }
    blocks
}

fn entry_blocks(entry: &Entry, blocks: &mut Vec<TimelineBlock>) {
    let time = time_col(entry);
    match &entry.kind {
        EntryKind::User(content) => {
            push_message_blocks(blocks, time, "You", Color::Green, content, entry)
        }
        EntryKind::Assistant(content) => {
            push_message_blocks(blocks, time, "Claude", Color::Cyan, content, entry)
        }
        EntryKind::System { subtype, .. } => {
            let label = subtype.clone().unwrap_or_default();
            blocks.push(TimelineBlock {
                time,
                kind: dim_line("system"),
                lines: vec![dim_line(label)],
                is_hook: false,
            });
        }
        EntryKind::Hook(summary) => {
            blocks.push(TimelineBlock {
                time,
                kind: Line::from(Span::styled(
                    "hook",
                    Style::default().fg(HOOK_COLOR).add_modifier(Modifier::BOLD),
                )),
                lines: hook_lines(summary),
                is_hook: true,
            });
        }
        EntryKind::Attachment { attachment_type } => {
            let label = attachment_type.clone().unwrap_or_else(|| "?".into());
            blocks.push(TimelineBlock {
                time,
                kind: dim_line("attachment"),
                lines: vec![dim_line(label)],
                is_hook: false,
            });
        }
        EntryKind::Unknown { type_name } => {
            // kind 列が空だと継続ブロック (空 kind) と見分けが付かないため、
            // type 欠落 (空文字) のときはプレースホルダを入れて先頭行を示す。
            let label = if type_name.is_empty() {
                "unknown"
            } else {
                type_name
            };
            blocks.push(TimelineBlock {
                time,
                kind: dim_line(label),
                lines: vec![Line::from("")],
                is_hook: false,
            });
        }
        EntryKind::Meta { .. } => {}
    }
}

/// user/assistant メッセージを表のブロック列へ展開する。エントリ先頭ブロックにのみ
/// time と kind (役割) を置き、以降は継続ブロック (time/kind 空) にする。中身が空の
/// content ブロックは飛ばし、全て空なら 1 ブロックも作らない。
fn push_message_blocks(
    blocks: &mut Vec<TimelineBlock>,
    time: String,
    role: &str,
    color: Color,
    content: &[ContentBlock],
    entry: &Entry,
) {
    let mut lines_per_block: Vec<Vec<Line<'static>>> = content
        .iter()
        .map(block_detail_lines)
        .filter(|lines| !lines.is_empty())
        .collect();
    if lines_per_block.is_empty() {
        return;
    }

    // kind 列は狭いので、attributionSkill バッジは先頭ブロックの先頭行の頭に付ける。
    if let Some(badge) = attribution_badge(entry) {
        let first_block = &mut lines_per_block[0];
        let first = first_block.remove(0);
        let mut spans = vec![badge, Span::raw(" ")];
        spans.extend(first.spans);
        first_block.insert(0, Line::from(spans));
    }

    let kind = Line::from(Span::styled(
        role.to_string(),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    ));
    for (i, lines) in lines_per_block.into_iter().enumerate() {
        blocks.push(TimelineBlock {
            time: if i == 0 { time.clone() } else { String::new() },
            kind: if i == 0 { kind.clone() } else { Line::from("") },
            lines,
            is_hook: false,
        });
    }
}

/// 1 ブロックを可視行 (Row) へ展開する。Detail 列は横幅でクリップされる (Table は
/// 折り返さない) ため、`detail_width` で明示的に折り返してから行にする。折り返し後が
/// 2 行以上なら折りたたみ対象で、既定は先頭行のみ表示しマーカー `[+N]` を付ける。
/// `expanded` のとき全行を出し `[-]` を付ける。継続行は time/kind を空にして表を詰める。
fn block_rows(block: &TimelineBlock, expanded: bool, detail_width: usize) -> Vec<Row<'static>> {
    // 各論理行を Detail 幅へ折り返した全表示行 (折りたたみ前)。
    let mut display = wrapped_lines(block, detail_width);

    let foldable = display.len() >= 2;
    let mut head = display.remove(0);
    if foldable {
        // マーカーぶんの幅を確保する。先頭行が満杯だとマーカーがクリップされ折りたたみ
        // の手掛かりが消えるため、必要なときだけ先頭行を再折り返しして末尾を後続へ回す。
        // 予約幅は再折り返し前の件数で見積もる (件数の桁が増えても内容は失われない)。
        let reserve = if expanded {
            " [-]".width()
        } else {
            format!(" [+{}]", display.len()).width()
        };
        if head.width() + reserve > detail_width {
            let mut pieces = wrap_line(&head, detail_width.saturating_sub(reserve)).into_iter();
            head = pieces.next().unwrap_or_else(|| Line::from(""));
            for (offset, extra) in pieces.enumerate() {
                display.insert(offset, extra);
            }
        }
        // マーカーは再折り返し後の隠れ行数で表示する (先頭行が割れた分も数える)。
        let marker_text = if expanded {
            " [-]".to_string()
        } else {
            format!(" [+{}]", display.len())
        };
        head.spans.push(Span::styled(marker_text, dim_style()));
    }

    let mut rows = vec![Row::new(vec![
        Cell::from(block.time.clone()),
        Cell::from(block.kind.clone()),
        Cell::from(head),
    ])];
    if foldable && expanded {
        for line in display {
            rows.push(Row::new(vec![
                Cell::from(String::new()),
                Cell::from(Line::from("")),
                Cell::from(line),
            ]));
        }
    }
    rows
}

/// ブロックの全論理行を Detail 幅 `detail_width` で折り返した表示行列 (折りたたみ前)。
/// 行数が 2 以上なら折りたたみ対象になる。
fn wrapped_lines(block: &TimelineBlock, detail_width: usize) -> Vec<Line<'static>> {
    block
        .lines
        .iter()
        .flat_map(|l| wrap_line(l, detail_width))
        .collect()
}

/// スタイル付きの 1 行を表示幅 `width` で折り返す (全角は 2 幅として数える)。
/// Table は折り返さずクリップするだけなので、全文を確認できるようここで分割する。
/// `width` が 0 のときは分割せずそのまま返す (退化ケース、極小端末)。
fn wrap_line(line: &Line<'static>, width: usize) -> Vec<Line<'static>> {
    if width == 0 {
        return vec![line.clone()];
    }
    let mut out: Vec<Line<'static>> = Vec::new();
    let mut cur: Vec<Span<'static>> = Vec::new();
    let mut cur_width = 0usize;
    for span in &line.spans {
        let style = span.style;
        let mut buf = String::new();
        for ch in span.content.chars() {
            let cw = ch.width().unwrap_or(0);
            // 1 行に載り切らない位置で改行する。行頭 (cur_width==0) では 1 文字が幅を
            // 超えても改行しない (無限ループ回避。極端に狭い幅での軽微なはみ出しは許容)。
            if cur_width + cw > width && cur_width > 0 {
                if !buf.is_empty() {
                    cur.push(Span::styled(std::mem::take(&mut buf), style));
                }
                out.push(Line::from(std::mem::take(&mut cur)));
                cur_width = 0;
            }
            buf.push(ch);
            cur_width += cw;
        }
        if !buf.is_empty() {
            cur.push(Span::styled(buf, style));
        }
    }
    out.push(Line::from(cur));
    out
}

fn block_detail_lines(block: &ContentBlock) -> Vec<Line<'static>> {
    match block {
        ContentBlock::Text(text) => text_body_lines(text, Color::Reset),
        ContentBlock::Thinking(text) => {
            let mut lines = vec![dim_line("· thinking")];
            lines.extend(
                text_body_lines(text, Color::DarkGray)
                    .into_iter()
                    .map(|l| l.style(Style::default().add_modifier(Modifier::DIM))),
            );
            lines
        }
        ContentBlock::ToolUse {
            name,
            summary,
            skill,
        } => vec![tool_use_line(name, summary, skill.as_deref())],
        ContentBlock::ToolResult { summary } => {
            vec![dim_line(format!("↳ result: {summary}"))]
        }
    }
}

/// フック実行サマリを行へ展開する (cctrace の核: フックが意図通り起動したかの確認)。
/// 先頭行に「種別 ×数 / 実行時間 / 状態」を出し、エラーがあれば 1 件 1 行で続ける
/// (複数エラーは折りたたみ対象になる)。
fn hook_lines(summary: &HookSummary) -> Vec<Line<'static>> {
    let name = &summary.name;

    // `hookCount` が欠落 (0) でも `hookInfos` があれば件数が判る。実行時間の件数で補い
    // 「×0 なのに実行時間が出る」不整合を避ける。
    let count = if summary.hook_count == 0 {
        summary.durations_ms.len() as u64
    } else {
        summary.hook_count
    };
    // 1 実行の ×1 はノイズなので、複数実行時のみ件数を出す。
    let head_label = if count > 1 {
        format!("⚡ {name} ×{count}")
    } else {
        format!("⚡ {name}")
    };
    let mut spans = vec![Span::styled(
        head_label,
        Style::default().fg(HOOK_COLOR).add_modifier(Modifier::BOLD),
    )];

    // 「どのフックが発火したか」を先頭行で判別できるよう、コマンド (優先) または
    // 最初の注入内容をインラインで載せる。残りの注入内容は折りたたみ行に回す。
    let mut details: &[String] = &summary.details;
    let inline = if summary.command.is_some() {
        summary.command.clone()
    } else if let Some((first, rest)) = details.split_first() {
        details = rest;
        Some(first.clone())
    } else {
        None
    };
    if let Some(info) = inline {
        spans.push(Span::styled(
            format!("  {}", truncate_chars(&info, 60)),
            dim_style(),
        ));
    }

    let total_ms: u64 = summary.durations_ms.iter().sum();
    if !summary.durations_ms.is_empty() {
        spans.push(Span::styled(format!("  {total_ms}ms"), dim_style()));
    }

    // 状態: エラー > 継続阻止 > 正常。異常は赤で強調する。
    let status = if !summary.errors.is_empty() {
        Span::styled(
            format!("  ✗ {} error(s)", summary.errors.len()),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )
    } else if summary.prevented_continuation {
        Span::styled(
            "  ⛔ blocked",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled("  ✓ ok", Style::default().fg(Color::Green))
    };
    spans.push(status);

    let mut lines = vec![Line::from(spans)];
    for err in &summary.errors {
        lines.push(Line::from(Span::styled(
            format!("  ↳ {err}"),
            Style::default().fg(Color::Red),
        )));
    }
    // フックが注入した内容の残り (折りたたみ行)。
    for detail in details {
        lines.push(dim_line(format!("  ↳ {detail}")));
    }
    lines
}

/// インライン表示用に文字数上限で切り詰める (長いコマンドが行を占有しないように)。
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        let head: String = s.chars().take(max).collect();
        format!("{head}…")
    } else {
        s.to_owned()
    }
}

/// tool_use の 1 行。`Skill` 起動は SPEC §7 に従い強調する。
fn tool_use_line(name: &str, summary: &str, skill: Option<&str>) -> Line<'static> {
    if let Some(skill) = skill {
        return Line::from(vec![Span::styled(
            format!("✦ Skill: {skill}"),
            Style::default()
                .fg(SKILL_COLOR)
                .add_modifier(Modifier::BOLD),
        )]);
    }
    let text = if summary.is_empty() {
        format!("⚙ {name}")
    } else {
        format!("⚙ {name}  {summary}")
    };
    Line::from(Span::styled(text, Style::default().fg(Color::Yellow)))
}

/// 本文テキストを行に分割する (折り返しはせず改行で分割、長い行は描画時にクリップ)。
fn text_body_lines(text: &str, color: Color) -> Vec<Line<'static>> {
    text.lines()
        .map(|l| Line::from(Span::styled(l.to_string(), Style::default().fg(color))))
        .collect()
}

/// timestamp (ISO8601) から時刻列 `HH:MM:SS` を取り出す。欠落・異常時は空文字。
fn time_col(entry: &Entry) -> String {
    entry
        .timestamp
        .as_deref()
        .and_then(|ts| ts.split('T').nth(1))
        .map(|t| t.chars().take(8).collect())
        .unwrap_or_default()
}

/// attributionSkill が付いたエントリに「Skill 由来」バッジを作る (SPEC §7)。
fn attribution_badge(entry: &Entry) -> Option<Span<'static>> {
    entry.attribution_skill.as_ref().map(|skill| {
        Span::styled(
            format!("[skill:{skill}]"),
            Style::default()
                .fg(SKILL_COLOR)
                .add_modifier(Modifier::BOLD),
        )
    })
}

fn dim_style() -> Style {
    Style::default()
        .fg(Color::DarkGray)
        .add_modifier(Modifier::DIM)
}

fn dim_line(text: impl Into<String>) -> Line<'static> {
    Line::from(Span::styled(text.into(), dim_style()))
}

fn hint_line(text: &str) -> Paragraph<'static> {
    Paragraph::new(Line::from(Span::styled(
        format!(" {text}"),
        Style::default().fg(Color::DarkGray),
    )))
}

fn skipped_suffix(skipped: usize) -> String {
    if skipped == 0 {
        String::new()
    } else {
        format!(" (skipped {skipped})")
    }
}

/// mtime を `YYYY-MM-DD HH:MM` 相当の UTC 文字列に整形する (依存を増やさない簡易実装)。
fn format_mtime(time: std::time::SystemTime) -> String {
    let secs = time
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_unix_utc(secs)
}

/// Unix 秒を UTC の `YYYY-MM-DD HH:MM` に変換する。うるう秒は無視する。
fn format_unix_utc(secs: u64) -> String {
    let days = secs / 86_400;
    let tod = secs % 86_400;
    let (hh, mm) = (tod / 3600, (tod % 3600) / 60);
    let (y, mo, d) = civil_from_days(days as i64);
    format!("{y:04}-{mo:02}-{d:02} {hh:02}:{mm:02}")
}

/// Howard Hinnant のアルゴリズムで「1970-01-01 からの日数」を暦日に変換する。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::parse_jsonl;

    /// Line を可視テキストへ平坦化する (スタイルは無視、内容だけ検証する)。
    fn text_of(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    /// 全ブロックの全行を改行区切りのテキストへ平坦化する (内容だけ検証する)。
    fn joined(entries: &[Entry], show_branches: bool) -> String {
        build_blocks(entries, show_branches)
            .iter()
            .flat_map(|b| b.lines.iter())
            .map(text_of)
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn skill_invocation_is_highlighted() {
        let s = parse_jsonl(
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Skill","input":{"skill":"commit"}}]}}"#,
        );
        let out = joined(&s.entries, false);
        assert!(out.contains("✦ Skill: commit"), "got: {out}");
    }

    #[test]
    fn attribution_skill_shows_badge() {
        let s = parse_jsonl(
            r#"{"type":"assistant","attributionSkill":"commit","message":{"content":[{"type":"text","text":"done"}]}}"#,
        );
        let out = joined(&s.entries, false);
        assert!(out.contains("[skill:commit]"), "got: {out}");
    }

    #[test]
    fn only_hook_block_is_flagged_is_hook() {
        let s = parse_jsonl(
            "{\"type\":\"user\",\"message\":{\"content\":\"hi\"}}\n\
             {\"type\":\"system\",\"subtype\":\"stop_hook_summary\",\"hookCount\":1}\n\
             {\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"yo\"}]}}",
        );
        let blocks = build_blocks(&s.entries, false);
        let hook_count = blocks.iter().filter(|b| b.is_hook()).count();
        assert_eq!(hook_count, 1, "exactly one hook block expected");
        assert!(
            blocks
                .iter()
                .any(|b| b.is_hook() && text_of(&b.kind) == "hook"),
            "the hook block should carry the hook kind label"
        );
    }

    #[test]
    fn hook_summary_is_rendered_with_marker_and_count() {
        let s = parse_jsonl(
            r#"{"type":"system","subtype":"stop_hook_summary","hookCount":2,"hookInfos":[{"durationMs":100},{"durationMs":50}],"hookErrors":[],"preventedContinuation":false}"#,
        );
        let out = joined(&s.entries, false);
        assert!(out.contains('⚡'), "hook marker missing: {out}");
        assert!(out.contains("stop hook"), "hook kind missing: {out}");
        assert!(out.contains("×2"), "hook count missing: {out}");
    }

    #[test]
    fn hook_attachment_rendered_with_name_duration_and_details() {
        let s = parse_jsonl(
            "{\"type\":\"attachment\",\"attachment\":{\"type\":\"hook_success\",\"hookName\":\"PostToolUse:Write\",\"exitCode\":0,\"durationMs\":263,\"command\":\"nix fmt 2>/dev/null || true\"}}\n\
             {\"type\":\"attachment\",\"attachment\":{\"type\":\"hook_additional_context\",\"hookName\":\"PostToolUse:Edit\",\"content\":[\"fmt ran on /a/b.rs\"]}}",
        );
        let out = joined(&s.entries, false);
        assert!(out.contains("⚡ PostToolUse:Write"), "got: {out}");
        assert!(out.contains("263ms"), "got: {out}");
        assert!(out.contains("⚡ PostToolUse:Edit"), "got: {out}");
        assert!(out.contains("fmt ran on /a/b.rs"), "got: {out}");
        // どのフックかを示すコマンドが表示される。
        assert!(out.contains("nix fmt"), "command missing: {out}");
        // 1 実行しかない attachment 系に ×1 は出さない (ノイズ)。
        assert!(!out.contains("×1"), "needless x1: {out}");
    }

    #[test]
    fn hook_head_line_inlines_command_and_first_detail() {
        // hook_success はコマンドが先頭行に載る (折りたたみ不要で「何のフックか」が判る)。
        let s = parse_jsonl(
            r#"{"type":"attachment","attachment":{"type":"hook_success","hookName":"PostToolUse:Write","exitCode":0,"durationMs":263,"command":"nix fmt 2>/dev/null || true"}}"#,
        );
        let blocks = build_blocks(&s.entries, false);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].lines.len(), 1, "command should be inline");
        assert!(text_of(&blocks[0].lines[0]).contains("nix fmt"));

        // content 1 件だけの hook_system_message は内容が先頭行に載り、折りたたみ無し。
        let s = parse_jsonl(
            r#"{"type":"attachment","attachment":{"type":"hook_system_message","hookName":"PostToolUse:Edit","content":"nix fmt hook fired"}}"#,
        );
        let blocks = build_blocks(&s.entries, false);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].lines.len(), 1, "single detail should be inline");
        assert!(text_of(&blocks[0].lines[0]).contains("nix fmt hook fired"));
    }

    #[test]
    fn hook_count_falls_back_to_hookinfos_when_count_missing() {
        // hookCount 欠落・hookInfos のみでも検出される。件数は hookInfos の数で補う
        // (「×0 なのに実行時間が出る」不整合を避ける)。
        let s = parse_jsonl(
            r#"{"type":"system","subtype":"stop_hook_summary","hookInfos":[{"durationMs":30},{"durationMs":20}]}"#,
        );
        let out = joined(&s.entries, false);
        assert!(out.contains("×2"), "count fallback missing: {out}");
        assert!(out.contains("50ms"), "total duration missing: {out}");
    }

    #[test]
    fn hook_summary_with_errors_shows_error_text() {
        let s = parse_jsonl(
            r#"{"type":"system","subtype":"stop_hook_summary","hookCount":1,"hookErrors":["boom"],"preventedContinuation":false}"#,
        );
        let out = joined(&s.entries, false);
        assert!(out.contains("boom"), "error text missing: {out}");
    }

    #[test]
    fn hook_summary_prevented_continuation_shows_blocked() {
        let s = parse_jsonl(
            r#"{"type":"system","subtype":"stop_hook_summary","hookCount":1,"hookErrors":[],"preventedContinuation":true}"#,
        );
        let out = joined(&s.entries, false);
        assert!(out.contains("blocked"), "blocked marker missing: {out}");
    }

    #[test]
    fn sidechain_hidden_by_default_shown_when_toggled() {
        let s =
            parse_jsonl(r#"{"type":"user","isSidechain":true,"message":{"content":"side task"}}"#);
        assert!(!joined(&s.entries, false).contains("side task"));
        assert!(joined(&s.entries, true).contains("side task"));
    }

    #[test]
    fn meta_entries_are_never_rendered() {
        let s = parse_jsonl(r#"{"type":"ai-title","aiTitle":"secret"}"#);
        assert!(joined(&s.entries, true).is_empty());
    }

    #[test]
    fn entries_are_ordered_by_timestamp_ascending() {
        // 記録順は later→earlier だが、表示は timestamp 昇順になる (SPEC §6.2)。
        let s = parse_jsonl(
            "{\"type\":\"user\",\"timestamp\":\"2026-07-10T12:00:00Z\",\"message\":{\"content\":\"second\"}}\n\
             {\"type\":\"user\",\"timestamp\":\"2026-07-10T09:00:00Z\",\"message\":{\"content\":\"first\"}}",
        );
        let out = joined(&s.entries, false);
        let first_at = out.find("first").expect("first present");
        let second_at = out.find("second").expect("second present");
        assert!(
            first_at < second_at,
            "earlier timestamp should render first: {out}"
        );
    }

    #[test]
    fn empty_type_entry_gets_placeholder_kind_not_blank_row() {
        // type 欠落の Unknown が空 kind ブロックになり継続ブロックと混同されるのを防ぐ (レビュー指摘 #1)。
        let s = parse_jsonl(r#"{"foo":1}"#);
        let blocks = build_blocks(&s.entries, false);
        assert_eq!(blocks.len(), 1);
        assert_eq!(text_of(&blocks[0].kind), "unknown");
    }

    #[test]
    fn multiline_text_block_is_foldable() {
        let s = parse_jsonl(
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"a\nb\nc"}]}}"#,
        );
        let blocks = build_blocks(&s.entries, false);
        assert_eq!(blocks.len(), 1);
        // 十分広い幅では折り返しが起きず、論理行数がそのまま折りたたみ対象判定になる。
        assert_eq!(wrapped_lines(&blocks[0], 200).len(), 3);
    }

    #[test]
    fn single_line_tool_use_is_not_foldable() {
        let s = parse_jsonl(
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Read","input":{"file_path":"/a"}}]}}"#,
        );
        let blocks = build_blocks(&s.entries, false);
        assert_eq!(blocks.len(), 1);
        assert_eq!(wrapped_lines(&blocks[0], 200).len(), 1);
    }

    #[test]
    fn wrap_line_splits_by_display_width_keeping_all_content() {
        let line = Line::from("abcdefghij");
        let wrapped = wrap_line(&line, 4);
        // 幅 4 で 10 文字 → 4 + 4 + 2。
        assert_eq!(wrapped.len(), 3);
        for l in &wrapped {
            assert!(l.width() <= 4, "line exceeds width: {:?}", text_of(l));
        }
        let joined: String = wrapped.iter().map(text_of).collect();
        assert_eq!(joined, "abcdefghij", "wrapping must not drop content");
    }

    #[test]
    fn wrap_line_counts_fullwidth_as_two_columns() {
        // 全角 3 文字 (幅 6) を幅 4 で折り返すと 2 文字 + 1 文字。
        let line = Line::from("あいう");
        let wrapped = wrap_line(&line, 4);
        assert_eq!(wrapped.len(), 2);
        assert_eq!(text_of(&wrapped[0]), "あい");
        assert_eq!(text_of(&wrapped[1]), "う");
    }

    #[test]
    fn expanded_timeline_renders_long_line_without_clipping() {
        // draw で決まる Detail 幅への折り返しが実幅を超えないこと (超えるとクリップされ
        // 文字が失われる) を、実描画バッファで検証する。
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let text = "z".repeat(150);
        let s = parse_jsonl(&format!(
            r#"{{"type":"user","message":{{"content":"{text}"}}}}"#
        ));
        let mut app = App::new(vec![]);
        app.screen = Screen::Timeline;
        app.open = Some(crate::app::open_session_for_test("s", s));
        app.handle(crate::app::Action::Enter); // 選択ブロックを展開

        let mut terminal = Terminal::new(TestBackend::new(80, 40)).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();

        let rendered: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect();
        // ヘッダ・ヒントに 'z' は無いので、全 150 文字が見えていればクリップされていない。
        let z_count = rendered.chars().filter(|&c| c == 'z').count();
        assert_eq!(
            z_count, 150,
            "long line must not be clipped: found {z_count}"
        );
    }

    #[test]
    fn long_single_logical_line_becomes_foldable_when_wrapped() {
        // 改行を含まない長い 1 行 (これまで横クリップで続きが見えなかったケース)。
        let text = "z".repeat(120);
        let s = parse_jsonl(&format!(
            r#"{{"type":"user","message":{{"content":"{text}"}}}}"#
        ));
        let blocks = build_blocks(&s.entries, false);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].lines.len(), 1, "改行が無いので論理行は 1 行");
        // 狭い Detail 幅では折り返されて折りたたみ対象になり、全文を再構成できる。
        let wrapped = wrapped_lines(&blocks[0], 40);
        assert!(wrapped.len() >= 2, "narrow width should fold the long line");
        let joined: String = wrapped.iter().map(text_of).collect();
        assert_eq!(joined, text, "expanding must reveal the whole line");
    }

    #[test]
    fn only_first_block_of_entry_carries_time_and_kind() {
        // 2 つの content ブロックを持つエントリでは、2 つ目は継続ブロック (time/kind 空)。
        let s = parse_jsonl(
            r#"{"type":"assistant","timestamp":"2026-07-10T12:00:00Z","message":{"content":[{"type":"text","text":"hi"},{"type":"tool_use","name":"Read","input":{"file_path":"/a"}}]}}"#,
        );
        let blocks = build_blocks(&s.entries, false);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].time, "12:00:00");
        assert_eq!(text_of(&blocks[0].kind), "Claude");
        assert_eq!(blocks[1].time, "");
        assert_eq!(text_of(&blocks[1].kind), "");
    }

    #[test]
    fn empty_assistant_entry_produces_no_header() {
        let s = parse_jsonl(r#"{"type":"assistant","message":{"content":[]}}"#);
        assert!(joined(&s.entries, false).is_empty());
    }

    #[test]
    fn format_unix_utc_known_epoch() {
        // 2026-07-10T16:52:00Z = 1783702320
        assert_eq!(format_unix_utc(1_783_702_320), "2026-07-10 16:52");
        assert_eq!(format_unix_utc(0), "1970-01-01 00:00");
    }
}
