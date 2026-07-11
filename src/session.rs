//! JSONL セッションファイルの読み込み。壊れた行はスキップし、既知行のみを
//! `Entry` に変換する (SPEC §4.3 / §11)。

use std::fs;
use std::path::Path;

use crate::model::{Entry, parse_entry};

/// 1 セッション分のパース結果。エントリは JSONL の記録順を保つ。
#[derive(Debug, Clone)]
pub struct Session {
    pub entries: Vec<Entry>,
    /// JSON として解釈できず読み飛ばした行数。堅牢性の可視化に用いる。
    pub skipped_lines: usize,
}

/// ファイルパスからセッションを読み込む。IO エラーは呼び出し側へ返す。
pub fn load_session(path: &Path) -> std::io::Result<Session> {
    let text = fs::read_to_string(path)?;
    Ok(parse_jsonl(&text))
}

/// JSONL 文字列をパースする。空行と JSON パース失敗行はスキップする。
pub fn parse_jsonl(text: &str) -> Session {
    let mut entries = Vec::new();
    let mut skipped_lines = 0;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<serde_json::Value>(trimmed) {
            Ok(value) => entries.push(parse_entry(&value)),
            Err(_) => skipped_lines += 1,
        }
    }

    Session {
        entries,
        skipped_lines,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::EntryKind;

    #[test]
    fn parses_multiple_lines_in_order() {
        let text = r#"{"type":"user","uuid":"a","message":{"content":"hi"}}
{"type":"assistant","uuid":"b","message":{"content":[]}}"#;
        let s = parse_jsonl(text);
        assert_eq!(s.entries.len(), 2);
        assert_eq!(s.entries[0].uuid.as_deref(), Some("a"));
        assert_eq!(s.entries[1].uuid.as_deref(), Some("b"));
        assert_eq!(s.skipped_lines, 0);
    }

    #[test]
    fn skips_broken_and_empty_lines_without_panicking() {
        let text = "{\"type\":\"user\",\"message\":{\"content\":\"ok\"}}\n\
                    this is not json\n\
                    \n\
                    {\"type\":\"assistant\",\"message\":{\"content\":[]}}";
        let s = parse_jsonl(text);
        assert_eq!(s.entries.len(), 2);
        assert_eq!(s.skipped_lines, 1);
    }

    #[test]
    fn unknown_types_are_kept() {
        let s = parse_jsonl(r#"{"type":"future-thing","uuid":"z"}"#);
        assert_eq!(s.entries.len(), 1);
        assert!(matches!(s.entries[0].kind, EntryKind::Unknown { .. }));
    }

    #[test]
    fn empty_input_yields_empty_session() {
        let s = parse_jsonl("");
        assert!(s.entries.is_empty());
        assert_eq!(s.skipped_lines, 0);
    }
}
