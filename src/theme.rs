//! 配色テーマ。UI 全体の色をここに集約し、画面間で一貫した見た目を保つ。
//!
//! ANSI 16 色 (`Color::Blue` など) は端末テーマ依存で彩度が高く古い印象になるため、
//! ratatui 公式デモと同じ tailwind パレット (truecolor) を使う。
//! 本文テキストは意図的に色指定しない (`Color::Reset`) ことで端末の前景色に従う。

use ratatui::style::palette::tailwind;
use ratatui::style::{Color, Modifier, Style};

/// ブランドのアクセント色 (タイトル・選択バー)。Claude のオレンジに寄せる。
pub const ACCENT: Color = tailwind::ORANGE.c400;
/// 枠線。目立たせず内容を主役にする。
pub const BORDER: Color = tailwind::SLATE.c600;
/// セカンダリ情報 (時刻・メタ・ヒント説明)。
pub const MUTED: Color = tailwind::SLATE.c500;
/// 選択行の背景。派手な反転ではなく一段浮く程度に留める。
pub const SELECTION_BG: Color = tailwind::SLATE.c800;

/// user 発話のロール色。
pub const USER: Color = tailwind::EMERALD.c400;
/// assistant 発話のロール色。
pub const ASSISTANT: Color = tailwind::CYAN.c400;
/// Skill 起動・attributionSkill 由来の強調色。
pub const SKILL: Color = tailwind::FUCHSIA.c400;
/// フック起動の強調色 (cctrace の核)。Skill/tool と混ざらない色を選ぶ。
pub const HOOK: Color = tailwind::SKY.c400;
/// tool_use 行の色。
pub const TOOL: Color = tailwind::AMBER.c300;
/// 正常状態 (✓ ok)。
pub const OK: Color = tailwind::GREEN.c400;
/// 異常状態 (エラー・blocked)。
pub const ERROR: Color = tailwind::RED.c400;

/// セカンダリ情報のスタイル。
pub fn muted() -> Style {
    Style::default().fg(MUTED)
}

/// アクセント色の強調スタイル (タイトルなど)。
pub fn accent_bold() -> Style {
    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
}

/// 選択行のスタイル。文字色は変えず背景だけ一段浮かせる。
pub fn selection() -> Style {
    Style::default().bg(SELECTION_BG)
}
