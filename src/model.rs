//! セッションエントリのドメインモデルと、JSONL の 1 行 (`serde_json::Value`) を
//! ドメイン型へ変換するロジック。
//!
//! 前方互換のため、未知の `type` やフィールドはエラーにせず既知部分のみ解釈する
//! (SPEC §4.3)。壊れた行のスキップは呼び出し側 (`session`) が担う。

use serde_json::Value;

/// タイムラインに並ぶ 1 エントリ。JSONL の 1 行に対応する。
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub uuid: Option<String>,
    pub parent_uuid: Option<String>,
    pub timestamp: Option<String>,
    pub is_sidechain: bool,
    pub is_meta: bool,
    /// この操作を駆動した Skill 名 (SPEC §5.1)。付いていれば「Skill 由来」。
    pub attribution_skill: Option<String>,
    pub kind: EntryKind,
}

/// エントリの種別。表示ルール (SPEC §7) はこの種別で分岐する。
#[derive(Debug, Clone, PartialEq)]
pub enum EntryKind {
    User(Vec<Block>),
    Assistant(Vec<Block>),
    System {
        subtype: Option<String>,
        text: Option<String>,
    },
    /// フック実行サマリ (`system` の `*_hook_summary`)。現状 transcript に構造的な
    /// 痕跡を残すのは Stop フック (`stop_hook_summary`) のみ (SPEC §5.2)。
    Hook(HookSummary),
    Attachment {
        attachment_type: Option<String>,
    },
    /// ai-title / last-prompt / mode 等のメタ情報。MVP では既定で非表示。
    Meta {
        type_name: String,
    },
    /// 既知の変換規則に当てはまらない type。前方互換のため保持する。
    Unknown {
        type_name: String,
    },
}

/// メッセージ本文 (`message.content[]`) の 1 ブロック。
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Text(String),
    Thinking(String),
    /// ツール呼び出し。`skill` は `name == "Skill"` のとき起動対象 Skill 名。
    ToolUse {
        name: String,
        summary: String,
        skill: Option<String>,
    },
    /// ツール実行結果 (user メッセージ内)。要約のみ保持する。
    ToolResult {
        summary: String,
    },
}

/// フック実行サマリの構造化フィールド。cctrace の核 (Hook が意図通り起動したかの確認)。
///
/// 2 系統の記録元を吸収する:
/// - `system` の `stop_hook_summary` (Stop フック)
/// - `attachment` の `hook_*` (PostToolUse 等。フックが出力またはファイル変更した時に記録)
#[derive(Debug, Clone, PartialEq)]
pub struct HookSummary {
    /// 表示名。system 系は subtype 由来 (例: "stop hook")、attachment 系は
    /// hookName (例: "PostToolUse:Write")。
    pub name: String,
    /// 実行されたフック数 (`hookCount`)。attachment 系は 1 件 = 1 実行。
    pub hook_count: u64,
    /// 各フックの実行時間 (`hookInfos[].durationMs` / `durationMs`)。
    pub durations_ms: Vec<u64>,
    /// フックが報告したエラー (`hookErrors` / 非 0 の `exitCode` + stderr)。空なら正常。
    pub errors: Vec<String>,
    /// フックが継続を阻止したか (`preventedContinuation`)。
    pub prevented_continuation: bool,
    /// 実行されたコマンド (`command`)。どのフックが発火したかの識別に使う。
    pub command: Option<String>,
    /// フックが注入した内容の要約 (`content`)。折りたたみ行として表示する。
    pub details: Vec<String>,
}

impl EntryKind {
    /// 既定のタイムライン (主系列) で表示すべき種別か。メタ情報は既定で隠す。
    pub fn is_visible_by_default(&self) -> bool {
        !matches!(self, EntryKind::Meta { .. })
    }
}

/// JSONL の 1 行を `Entry` に変換する。未知フィールドは無視する。
pub fn parse_entry(value: &Value) -> Entry {
    let type_name = value.get("type").and_then(Value::as_str).unwrap_or("");

    let kind = match type_name {
        "user" => EntryKind::User(parse_content(value)),
        "assistant" => EntryKind::Assistant(parse_content(value)),
        // フック系 system は `hookCount` / `hookInfos` を持つ。専用種別に振り分ける。
        "system" if value.get("hookCount").is_some() || value.get("hookInfos").is_some() => {
            EntryKind::Hook(parse_hook_summary(value))
        }
        "system" => EntryKind::System {
            subtype: str_field(value, "subtype"),
            text: str_field(value, "content").or_else(|| str_field(value, "text")),
        },
        "attachment" => parse_attachment(value),
        "ai-title"
        | "custom-title"
        | "last-prompt"
        | "mode"
        | "permission-mode"
        | "pr-link"
        | "file-history-snapshot"
        | "queue-operation" => EntryKind::Meta {
            type_name: type_name.to_owned(),
        },
        other => EntryKind::Unknown {
            type_name: other.to_owned(),
        },
    };

    Entry {
        uuid: str_field(value, "uuid"),
        parent_uuid: str_field(value, "parentUuid"),
        timestamp: str_field(value, "timestamp"),
        is_sidechain: value
            .get("isSidechain")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        is_meta: value
            .get("isMeta")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        attribution_skill: str_field(value, "attributionSkill"),
        kind,
    }
}

/// attachment エントリを解釈する。フック実行記録 (`attachment.type == "hook_*"`) は
/// 専用の Hook 種別へ、それ以外は従来通り Attachment へ振り分ける。
fn parse_attachment(value: &Value) -> EntryKind {
    let attachment = value.get("attachment");
    let attachment_type = attachment
        .and_then(|a| a.get("type"))
        .and_then(Value::as_str);
    if let (Some(a), Some(t)) = (attachment, attachment_type)
        && t.starts_with("hook_")
    {
        return EntryKind::Hook(parse_hook_attachment(a, t));
    }
    EntryKind::Attachment {
        attachment_type: attachment_type.map(str::to_owned),
    }
}

/// `hook_success` / `hook_error` / `hook_system_message` / `hook_additional_context` 等の
/// attachment を `HookSummary` へ変換する。未知・欠落フィールドは既定値へ (SPEC §4.3)。
fn parse_hook_attachment(attachment: &Value, attachment_type: &str) -> HookSummary {
    let name = attachment
        .get("hookName")
        .and_then(Value::as_str)
        .unwrap_or(attachment_type)
        .to_owned();
    // attachment 系は 1 記録 = 1 実行。durationMs は hook_success にのみ載る。
    let durations_ms = attachment
        .get("durationMs")
        .and_then(Value::as_u64)
        .into_iter()
        .collect();
    let mut errors = Vec::new();
    if let Some(code) = attachment.get("exitCode").and_then(Value::as_i64)
        && code != 0
    {
        let stderr = attachment
            .get("stderr")
            .and_then(Value::as_str)
            .unwrap_or("");
        errors.push(if stderr.is_empty() {
            format!("exit code {code}")
        } else {
            one_line(stderr)
        });
    }
    // content はフックが注入した内容 (文字列または文字列配列)。
    let details = match attachment.get("content") {
        Some(Value::String(s)) if !s.is_empty() => vec![one_line(s)],
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(one_line)
            .collect(),
        _ => Vec::new(),
    };
    HookSummary {
        name,
        hook_count: 1,
        durations_ms,
        errors,
        prevented_continuation: false,
        command: attachment
            .get("command")
            .and_then(Value::as_str)
            .map(one_line),
        details,
    }
}

/// フック系 system エントリの構造化フィールドを `HookSummary` へ変換する。
/// 未知・欠落フィールドは既定値にフォールバックする (前方互換、SPEC §4.3)。
fn parse_hook_summary(value: &Value) -> HookSummary {
    let durations_ms = value
        .get("hookInfos")
        .and_then(Value::as_array)
        .map(|infos| {
            infos
                .iter()
                .filter_map(|i| i.get("durationMs").and_then(Value::as_u64))
                .collect()
        })
        .unwrap_or_default();
    let errors = value
        .get("hookErrors")
        .and_then(Value::as_array)
        .map(|errs| {
            errs.iter()
                // 文字列ならそのまま、そうでなければ JSON 表現に畳んで捨てない。
                .map(|e| {
                    e.as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| e.to_string())
                })
                .collect()
        })
        .unwrap_or_default();
    // `stop_hook_summary` → `stop hook` のように読みやすい表示名にする。
    let subtype = str_field(value, "subtype").unwrap_or_default();
    let name = subtype.trim_end_matches("_summary").replace('_', " ");
    let name = if name.is_empty() { "hook".into() } else { name };
    HookSummary {
        name,
        hook_count: value.get("hookCount").and_then(Value::as_u64).unwrap_or(0),
        durations_ms,
        errors,
        prevented_continuation: value
            .get("preventedContinuation")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        command: None,
        details: Vec::new(),
    }
}

/// `message.content` を解釈する。content は文字列 (単純発話) か配列 (ブロック列)。
fn parse_content(value: &Value) -> Vec<Block> {
    let Some(content) = value.get("message").and_then(|m| m.get("content")) else {
        return Vec::new();
    };

    match content {
        Value::String(s) => vec![Block::Text(s.clone())],
        Value::Array(items) => items.iter().filter_map(parse_block).collect(),
        _ => Vec::new(),
    }
}

/// content 配列の 1 要素をブロックに変換する。未知の要素型は無視する。
fn parse_block(item: &Value) -> Option<Block> {
    match item.get("type").and_then(Value::as_str)? {
        "text" => Some(Block::Text(
            item.get("text").and_then(Value::as_str)?.to_owned(),
        )),
        "thinking" => Some(Block::Thinking(
            item.get("thinking").and_then(Value::as_str)?.to_owned(),
        )),
        "tool_use" => {
            let name = item.get("name").and_then(Value::as_str)?.to_owned();
            let input = item.get("input");
            let skill = if name == "Skill" {
                input
                    .and_then(|i| i.get("skill"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            } else {
                None
            };
            Some(Block::ToolUse {
                summary: summarize_tool_input(input),
                name,
                skill,
            })
        }
        "tool_result" => Some(Block::ToolResult {
            summary: summarize_tool_result(item.get("content")),
        }),
        _ => None,
    }
}

/// ツール入力を 1 行要約する (SPEC §7 の「主要 input を 1 行要約」)。
fn summarize_tool_input(input: Option<&Value>) -> String {
    let Some(input) = input else {
        return String::new();
    };
    // 代表的なツールの主要引数を優先的に拾う。無ければキー一覧にフォールバック。
    for key in [
        "skill",
        "file_path",
        "command",
        "pattern",
        "path",
        "description",
    ] {
        if let Some(v) = input.get(key).and_then(Value::as_str) {
            return one_line(v);
        }
    }
    match input {
        Value::Object(map) => map.keys().cloned().collect::<Vec<_>>().join(", "),
        _ => String::new(),
    }
}

/// tool_result の content を短い要約にする。content は文字列か配列。
fn summarize_tool_result(content: Option<&Value>) -> String {
    let text = match content {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|i| i.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    };
    one_line(&text)
}

/// 複数行テキストを 1 行の要約に畳む。長い場合は末尾を省略する。
///
/// ツール入力の要約 (本モジュール) と、一覧のタイトル整形 (`project`) で共用する。
/// 非空白の制御文字 (ESC 等) は端末表示を壊すため除去し、空白は 1 個に畳む。
pub fn one_line(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .filter(|c| !c.is_control() || c.is_whitespace())
        .collect();
    let flat = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX: usize = 120;
    if flat.chars().count() > MAX {
        let head: String = flat.chars().take(MAX).collect();
        format!("{head}…")
    } else {
        flat
    }
}

fn str_field(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_user_text_string_content() {
        let v = json!({"type": "user", "uuid": "u1", "message": {"content": "hello"}});
        let e = parse_entry(&v);
        assert_eq!(e.uuid.as_deref(), Some("u1"));
        assert_eq!(e.kind, EntryKind::User(vec![Block::Text("hello".into())]));
    }

    #[test]
    fn parses_assistant_blocks_text_thinking_tooluse() {
        let v = json!({
            "type": "assistant",
            "message": {"content": [
                {"type": "thinking", "thinking": "let me think"},
                {"type": "text", "text": "answer"},
                {"type": "tool_use", "name": "Read", "input": {"file_path": "/a/b.rs"}},
            ]},
        });
        let EntryKind::Assistant(blocks) = parse_entry(&v).kind else {
            panic!("expected assistant");
        };
        assert_eq!(blocks[0], Block::Thinking("let me think".into()));
        assert_eq!(blocks[1], Block::Text("answer".into()));
        assert_eq!(
            blocks[2],
            Block::ToolUse {
                name: "Read".into(),
                summary: "/a/b.rs".into(),
                skill: None,
            }
        );
    }

    #[test]
    fn skill_tool_use_captures_skill_name() {
        let v = json!({
            "type": "assistant",
            "message": {"content": [
                {"type": "tool_use", "name": "Skill", "input": {"skill": "commit"}},
            ]},
        });
        let EntryKind::Assistant(blocks) = parse_entry(&v).kind else {
            panic!("expected assistant");
        };
        assert_eq!(
            blocks[0],
            Block::ToolUse {
                name: "Skill".into(),
                summary: "commit".into(),
                skill: Some("commit".into()),
            }
        );
    }

    #[test]
    fn captures_attribution_skill() {
        let v =
            json!({"type": "assistant", "attributionSkill": "commit", "message": {"content": []}});
        assert_eq!(parse_entry(&v).attribution_skill.as_deref(), Some("commit"));
    }

    #[test]
    fn parses_attachment_type() {
        let v = json!({"type": "attachment", "attachment": {"type": "skill_listing"}});
        assert_eq!(
            parse_entry(&v).kind,
            EntryKind::Attachment {
                attachment_type: Some("skill_listing".into())
            }
        );
    }

    #[test]
    fn meta_types_are_hidden_by_default() {
        for t in ["ai-title", "last-prompt", "mode", "pr-link"] {
            let v = json!({"type": t});
            assert!(!parse_entry(&v).kind.is_visible_by_default(), "type={t}");
        }
    }

    #[test]
    fn unknown_type_is_preserved_not_panicking() {
        let v = json!({"type": "brand-new-type", "uuid": "x"});
        assert_eq!(
            parse_entry(&v).kind,
            EntryKind::Unknown {
                type_name: "brand-new-type".into()
            }
        );
    }

    #[test]
    fn user_tool_result_summarized() {
        let v = json!({
            "type": "user",
            "message": {"content": [
                {"type": "tool_result", "content": "line1\n   line2   \nline3"},
            ]},
        });
        let EntryKind::User(blocks) = parse_entry(&v).kind else {
            panic!("expected user");
        };
        assert_eq!(
            blocks[0],
            Block::ToolResult {
                summary: "line1 line2 line3".into()
            }
        );
    }

    #[test]
    fn one_line_strips_control_chars_and_flattens_whitespace() {
        // ESC/BEL 等の制御文字は除去、改行・タブ・連続空白は 1 個の空白に畳む。
        assert_eq!(one_line("a\x1bb\x07c\td\n\ne"), "abc d e");
    }

    #[test]
    fn parses_stop_hook_summary_into_hook_kind() {
        let v = json!({
            "type": "system",
            "subtype": "stop_hook_summary",
            "hookCount": 1,
            "hookInfos": [{"command": "x", "promptText": "x", "durationMs": 12690}],
            "hookErrors": [],
            "preventedContinuation": false,
        });
        let EntryKind::Hook(h) = parse_entry(&v).kind else {
            panic!("expected hook");
        };
        assert_eq!(h.name, "stop hook");
        assert_eq!(h.hook_count, 1);
        assert_eq!(h.durations_ms, vec![12690]);
        assert!(h.errors.is_empty());
        assert!(!h.prevented_continuation);
    }

    #[test]
    fn parses_hook_success_attachment_into_hook_kind() {
        // PostToolUse 等のフックが出力を持つと attachment.type == "hook_success" が記録される。
        let v = json!({
            "type": "attachment",
            "attachment": {
                "type": "hook_success",
                "hookName": "PostToolUse:Write",
                "hookEvent": "PostToolUse",
                "stdout": "{}",
                "stderr": "",
                "exitCode": 0,
                "durationMs": 263,
                "content": "",
                "command": "nix fmt 2>/dev/null || true",
            },
        });
        let EntryKind::Hook(h) = parse_entry(&v).kind else {
            panic!("expected hook");
        };
        assert_eq!(h.name, "PostToolUse:Write");
        assert_eq!(h.hook_count, 1);
        assert_eq!(h.durations_ms, vec![263]);
        assert_eq!(h.command.as_deref(), Some("nix fmt 2>/dev/null || true"));
        assert!(h.errors.is_empty());
        assert!(h.details.is_empty());
    }

    #[test]
    fn hook_attachment_nonzero_exit_becomes_error() {
        let v = json!({
            "type": "attachment",
            "attachment": {
                "type": "hook_error",
                "hookName": "PostToolUse:Edit",
                "exitCode": 2,
                "stderr": "boom",
            },
        });
        let EntryKind::Hook(h) = parse_entry(&v).kind else {
            panic!("expected hook");
        };
        assert_eq!(h.errors, vec!["boom".to_string()]);
    }

    #[test]
    fn hook_additional_context_content_becomes_details() {
        let v = json!({
            "type": "attachment",
            "attachment": {
                "type": "hook_additional_context",
                "hookName": "PostToolUse:Edit",
                "hookEvent": "PostToolUse",
                "content": ["PostToolUse hook modified /a/b.rs after your edit"],
            },
        });
        let EntryKind::Hook(h) = parse_entry(&v).kind else {
            panic!("expected hook");
        };
        assert_eq!(h.name, "PostToolUse:Edit");
        assert_eq!(
            h.details,
            vec!["PostToolUse hook modified /a/b.rs after your edit".to_string()]
        );
    }

    #[test]
    fn non_hook_attachment_stays_attachment() {
        let v = json!({"type": "attachment", "attachment": {"type": "task_reminder"}});
        assert!(matches!(parse_entry(&v).kind, EntryKind::Attachment { .. }));
    }

    #[test]
    fn hook_errors_and_prevented_continuation_are_captured() {
        let v = json!({
            "type": "system",
            "subtype": "stop_hook_summary",
            "hookCount": 2,
            "hookErrors": ["boom", "kaboom"],
            "preventedContinuation": true,
        });
        let EntryKind::Hook(h) = parse_entry(&v).kind else {
            panic!("expected hook");
        };
        assert_eq!(h.errors, vec!["boom".to_string(), "kaboom".to_string()]);
        assert!(h.prevented_continuation);
    }

    #[test]
    fn plain_system_without_hook_fields_stays_system() {
        let v = json!({"type": "system", "subtype": "turn_duration", "content": "1.2s"});
        assert!(matches!(parse_entry(&v).kind, EntryKind::System { .. }));
    }

    #[test]
    fn hook_kind_is_visible_by_default() {
        let v = json!({"type": "system", "subtype": "stop_hook_summary", "hookCount": 1});
        assert!(parse_entry(&v).kind.is_visible_by_default());
    }

    #[test]
    fn is_sidechain_and_meta_flags() {
        let v = json!({"type": "user", "isSidechain": true, "isMeta": true, "message": {"content": "x"}});
        let e = parse_entry(&v);
        assert!(e.is_sidechain);
        assert!(e.is_meta);
    }
}
