//! 対象プロジェクトの特定とセッションファイルの列挙 (SPEC §4.1 / §6.1)。
//!
//! パス探索は cwd 由来のディレクトリに限定し、読み取り専用で扱う (SPEC §9)。

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::Value;

use crate::model::one_line;

/// セッション一覧の 1 項目。一覧で選びやすくするため、stat に加えて軽量な
/// タイトルを持つ。本文の解釈は選択時に `session::load_session` で行う。
/// タイトルは `discover_sessions` が各ファイルを 1 度走査して埋める。
#[derive(Debug, Clone)]
pub struct SessionMeta {
    /// ファイル名の stem (= session-id)。
    pub id: String,
    pub path: PathBuf,
    pub modified: SystemTime,
    /// 一覧に出す短いタイトル。customTitle > aiTitle > 最初の user 発話の順。
    /// いずれも無ければ None。
    pub title: Option<String>,
}

/// cwd を Claude Code のプロジェクトディレクトリ名にエンコードする。
///
/// 観測された規則に従い `/` と `.` を `-` に置換する
/// (例: `/home/u/src/github.com/x` → `-home-u-src-github-com-x`)。
pub fn encode_cwd(cwd: &Path) -> String {
    cwd.to_string_lossy()
        .chars()
        .map(|c| if c == '/' || c == '.' { '-' } else { c })
        .collect()
}

/// `~/.claude/projects/<encoded cwd>` を組み立てる。
pub fn project_dir(claude_home: &Path, cwd: &Path) -> PathBuf {
    claude_home.join("projects").join(encode_cwd(cwd))
}

/// プロジェクトディレクトリ配下の `*.jsonl` を更新日時の新しい順で列挙する。
///
/// ディレクトリ不在は「空」として空 Vec を返す (エラーにしない、SPEC §6.1)。
pub fn discover_sessions(claude_home: &Path, cwd: &Path) -> Vec<SessionMeta> {
    let dir = project_dir(claude_home, cwd);
    let Ok(read_dir) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };

    let mut sessions: Vec<SessionMeta> = read_dir
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                return None;
            }
            let id = path.file_stem()?.to_string_lossy().into_owned();
            let modified = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            let title = read_title(&path);
            Some(SessionMeta {
                id,
                path,
                modified,
                title,
            })
        })
        .collect();

    sessions.sort_by_key(|s| std::cmp::Reverse(s.modified));
    sessions
}

/// セッションファイルを走査して一覧用タイトルを作る。読み込み失敗は None。
fn read_title(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    title_from_jsonl(&text)
}

/// JSONL から一覧用タイトルを抽出する。壊れた行・空行はスキップする。
///
/// customTitle > aiTitle > 最初の user 発話の優先順で採り、title 系は最新の行
/// (最後の出現) を採用する。整形後 (`one_line`) に空になる候補は飛ばして次へ
/// フォールバックし、すべて空/不在なら None。
fn title_from_jsonl(text: &str) -> Option<String> {
    let mut custom_title = None;
    let mut ai_title = None;
    let mut first_user = None;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        match value.get("type").and_then(Value::as_str) {
            Some("custom-title") => {
                if let Some(t) = value.get("customTitle").and_then(Value::as_str) {
                    custom_title = Some(t.to_owned());
                }
            }
            Some("ai-title") => {
                if let Some(t) = value.get("aiTitle").and_then(Value::as_str) {
                    ai_title = Some(t.to_owned());
                }
            }
            Some("user") if first_user.is_none() => {
                first_user = first_user_text(&value);
            }
            _ => {}
        }
    }

    // 優先順に整形し、空でない最初の候補を採る (上位が空白のみでも下位へ委ねる)。
    [custom_title, ai_title, first_user]
        .into_iter()
        .flatten()
        .map(|t| one_line(&t))
        .find(|t| !t.is_empty())
}

/// user エントリ本文から最初のテキストを取り出す。content は文字列か配列
/// (配列のときは最初の text ブロック)。tool_result のみの行は None。
fn first_user_text(value: &Value) -> Option<String> {
    let content = value.get("message")?.get("content")?;
    match content {
        Value::String(s) => Some(s.clone()),
        Value::Array(items) => items.iter().find_map(|item| {
            if item.get("type").and_then(Value::as_str)? == "text" {
                item.get("text").and_then(Value::as_str).map(str::to_owned)
            } else {
                None
            }
        }),
        _ => None,
    }
}

/// `$HOME/.claude` を返す。HOME 未設定時は None。
pub fn claude_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".claude"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::Duration;

    #[test]
    fn encodes_slashes_and_dots_to_hyphen() {
        assert_eq!(
            encode_cwd(Path::new("/home/myuron/src/github.com/myuron/cctrace")),
            "-home-myuron-src-github-com-myuron-cctrace"
        );
    }

    #[test]
    fn preserves_existing_hyphens() {
        assert_eq!(
            encode_cwd(Path::new("/home/u/Kyure-A/.emacs.d")),
            "-home-u-Kyure-A--emacs-d"
        );
    }

    #[test]
    fn missing_project_dir_yields_empty() {
        let tmp = std::env::temp_dir().join("cctrace-test-missing");
        let _ = fs::remove_dir_all(&tmp);
        let sessions = discover_sessions(&tmp, Path::new("/nope/nowhere"));
        assert!(sessions.is_empty());
    }

    #[test]
    fn lists_jsonl_sorted_by_mtime_desc() {
        let tmp = std::env::temp_dir().join(format!("cctrace-test-{}", std::process::id()));
        let cwd = Path::new("/work/proj");
        let dir = project_dir(&tmp, cwd);
        fs::create_dir_all(&dir).unwrap();

        let old = dir.join("old.jsonl");
        let new = dir.join("new.jsonl");
        fs::write(&old, "{}").unwrap();
        fs::write(&new, "{}").unwrap();
        // new のほうが後の mtime になるよう明示的に設定する。
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        filetime_set(&old, base);
        filetime_set(&new, base + Duration::from_secs(100));
        // 無関係な拡張子は無視される。
        fs::write(dir.join("note.txt"), "x").unwrap();

        let sessions = discover_sessions(&tmp, cwd);
        let ids: Vec<_> = sessions.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["new", "old"]);

        let _ = fs::remove_dir_all(&tmp);
    }

    /// mtime を確定的に設定する小さなヘルパ (テスト専用、標準 API のみ)。
    fn filetime_set(path: &Path, time: SystemTime) {
        let file = fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_modified(time).unwrap();
    }

    #[test]
    fn title_prefers_custom_title() {
        let text = "{\"type\":\"user\",\"message\":{\"content\":\"first prompt\"}}\n\
                    {\"type\":\"ai-title\",\"aiTitle\":\"AI made title\"}\n\
                    {\"type\":\"custom-title\",\"customTitle\":\"my title\"}";
        assert_eq!(title_from_jsonl(text).as_deref(), Some("my title"));
    }

    #[test]
    fn title_falls_back_to_ai_title_then_user_prompt() {
        let ai = title_from_jsonl(
            "{\"type\":\"ai-title\",\"aiTitle\":\"gen\"}\n\
             {\"type\":\"user\",\"message\":{\"content\":\"hi\"}}",
        );
        assert_eq!(ai.as_deref(), Some("gen"));

        let user = title_from_jsonl(r#"{"type":"user","message":{"content":"just a prompt"}}"#);
        assert_eq!(user.as_deref(), Some("just a prompt"));
    }

    #[test]
    fn title_uses_latest_occurrence() {
        let text = "{\"type\":\"ai-title\",\"aiTitle\":\"old\"}\n\
                    {\"type\":\"ai-title\",\"aiTitle\":\"new\"}";
        assert_eq!(title_from_jsonl(text).as_deref(), Some("new"));
    }

    #[test]
    fn title_from_first_user_text_block_array() {
        // 配列 content でも tool_result を飛ばして最初の text ブロックを採る。
        let text = r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"x"},{"type":"text","text":"real question"}]}}"#;
        assert_eq!(title_from_jsonl(text).as_deref(), Some("real question"));
    }

    #[test]
    fn blank_upper_title_falls_back_to_lower() {
        // customTitle が空白のみでも、有効な aiTitle にフォールバックする (指摘 #1)。
        let text = "{\"type\":\"custom-title\",\"customTitle\":\"   \"}\n\
                    {\"type\":\"ai-title\",\"aiTitle\":\"real title\"}";
        assert_eq!(title_from_jsonl(text).as_deref(), Some("real title"));
    }

    #[test]
    fn no_title_when_absent_and_broken_lines_skipped() {
        let text = "not json\n{\"type\":\"assistant\",\"message\":{\"content\":[]}}";
        assert_eq!(title_from_jsonl(text), None);
    }
}
