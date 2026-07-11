//! アプリケーション状態と操作。UI 描画 (`ui`) と端末 IO (`main`) から分離し、
//! キー操作 → 状態遷移のロジックを単体でテストできるようにする。

#[cfg(test)]
use std::path::Path;

use ratatui::widgets::TableState;

use crate::project::SessionMeta;
use crate::session::{self, Session};
use crate::ui;

/// 画面。SPEC §8 の「セッション一覧」→「タイムライン」の 2 画面。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    SessionList,
    Timeline,
}

/// 抽象化したキー操作。端末依存の crossterm イベントは `main` で本 enum へ写像する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Up,
    Down,
    Top,
    Bottom,
    Enter,
    Back,
    ToggleBranches,
    /// 次/前のフック起動ブロックへジャンプする (発見性向上)。
    NextHook,
    PrevHook,
    Quit,
}

/// 開いているセッションの状態 (本文 + 選択・展開状態)。
pub struct OpenSession {
    pub meta: SessionMeta,
    pub data: Session,
    /// 描画用にキャッシュしたタイムライン表のブロック。開いた時と分岐トグル時のみ
    /// 再構築し、毎キー入力での再変換 (大きなセッションでの無駄なアロケーション) を避ける。
    pub blocks: Vec<ui::TimelineBlock>,
    /// 選択中ブロックの添字 (↑/↓ で移動、Enter で開閉)。
    pub selected: usize,
    /// 各ブロックの展開状態 (blocks と同じ長さ)。折りたたみ対象のみ意味を持つ。
    pub expanded: Vec<bool>,
    /// 選択追従スクロールを Ratatui に委ねるための描画状態。
    pub table_state: TableState,
}

impl OpenSession {
    fn new(meta: SessionMeta, data: Session, show_branches: bool) -> Self {
        let blocks = ui::build_blocks(&data.entries, show_branches);
        let expanded = vec![false; blocks.len()];
        Self {
            meta,
            data,
            blocks,
            selected: 0,
            expanded,
            table_state: TableState::default(),
        }
    }

    /// 分岐表示の切替に伴いタイムライン表のブロックを作り直す。添字がずれるため
    /// 選択・展開・スクロールは初期化する。
    fn rebuild(&mut self, show_branches: bool) {
        self.blocks = ui::build_blocks(&self.data.entries, show_branches);
        self.expanded = vec![false; self.blocks.len()];
        self.selected = 0;
        self.table_state = TableState::default();
    }

    fn select_prev(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    fn select_next(&mut self) {
        if self.selected + 1 < self.blocks.len() {
            self.selected += 1;
        }
    }

    fn select_last(&mut self) {
        self.selected = self.blocks.len().saturating_sub(1);
    }

    /// 選択を次のフックブロックへ移す。末尾の先には先頭側へ巡回する。
    /// フックが無ければ選択は動かさない。
    fn select_next_hook(&mut self) {
        let n = self.blocks.len();
        for step in 1..=n {
            let i = (self.selected + step) % n;
            if self.blocks[i].is_hook() {
                self.selected = i;
                return;
            }
        }
    }

    /// 選択を前のフックブロックへ移す。先頭の手前は末尾側へ巡回する。
    fn select_prev_hook(&mut self) {
        let n = self.blocks.len();
        for step in 1..=n {
            let i = (self.selected + n - step) % n;
            if self.blocks[i].is_hook() {
                self.selected = i;
                return;
            }
        }
    }

    /// 選択中ブロックが折りたたみ対象なら展開/折りたたみをトグルする。
    fn toggle_selected(&mut self) {
        if self
            .blocks
            .get(self.selected)
            .is_some_and(ui::TimelineBlock::is_foldable)
            && let Some(e) = self.expanded.get_mut(self.selected)
        {
            *e = !*e;
        }
    }
}

pub struct App {
    pub sessions: Vec<SessionMeta>,
    pub list_selected: usize,
    pub screen: Screen,
    pub open: Option<OpenSession>,
    /// 分岐・サイドチェーンの表示 ON/OFF (SPEC §7)。既定 OFF。
    pub show_branches: bool,
    pub should_quit: bool,
    /// 直近の非致命エラー (例: セッション読み込み失敗)。UI 下部に表示する。
    pub status: Option<String>,
}

impl App {
    pub fn new(sessions: Vec<SessionMeta>) -> Self {
        Self {
            sessions,
            list_selected: 0,
            screen: Screen::SessionList,
            open: None,
            show_branches: false,
            should_quit: false,
            status: None,
        }
    }

    /// 抽象キー操作を状態へ反映する。端末非依存なので単体テストできる。
    pub fn handle(&mut self, action: Action) {
        match self.screen {
            Screen::SessionList => self.handle_list(action),
            Screen::Timeline => self.handle_timeline(action),
        }
    }

    fn handle_list(&mut self, action: Action) {
        match action {
            Action::Up => self.list_selected = self.list_selected.saturating_sub(1),
            Action::Down => {
                if self.list_selected + 1 < self.sessions.len() {
                    self.list_selected += 1;
                }
            }
            Action::Top => self.list_selected = 0,
            Action::Bottom => {
                self.list_selected = self.sessions.len().saturating_sub(1);
            }
            Action::Enter => self.open_selected(),
            Action::Back | Action::Quit => self.should_quit = true,
            Action::ToggleBranches | Action::NextHook | Action::PrevHook => {}
        }
    }

    fn handle_timeline(&mut self, action: Action) {
        match action {
            // ↑/↓ は選択ブロックを移動する。上限クランプは各ヘルパ内で行う。
            Action::Up => self.with_open(OpenSession::select_prev),
            Action::Down => self.with_open(OpenSession::select_next),
            Action::Top => self.with_open(|o| o.selected = 0),
            Action::Bottom => self.with_open(OpenSession::select_last),
            // 選択中ブロックが折りたたみ対象なら開閉する。
            Action::Enter => self.with_open(OpenSession::toggle_selected),
            Action::NextHook => self.with_open(OpenSession::select_next_hook),
            Action::PrevHook => self.with_open(OpenSession::select_prev_hook),
            Action::ToggleBranches => {
                self.show_branches = !self.show_branches;
                let show_branches = self.show_branches;
                self.with_open(|o| o.rebuild(show_branches));
            }
            Action::Back => {
                self.screen = Screen::SessionList;
                self.open = None;
            }
            Action::Quit => self.should_quit = true,
        }
    }

    fn with_open(&mut self, f: impl FnOnce(&mut OpenSession)) {
        if let Some(open) = &mut self.open {
            f(open);
        }
    }

    fn open_selected(&mut self) {
        let Some(meta) = self.sessions.get(self.list_selected).cloned() else {
            return;
        };
        match session::load_session(&meta.path) {
            Ok(data) => {
                self.open = Some(OpenSession::new(meta, data, self.show_branches));
                self.screen = Screen::Timeline;
                self.status = None;
            }
            Err(e) => {
                self.status = Some(format!("failed to open {}: {e}", meta.path.display()));
            }
        }
    }
}

/// 読み込み済みセッション向けの薄いテスト用コンストラクタ (端末なしで検証する)。
#[cfg(test)]
pub fn open_session_for_test(id: &str, data: Session) -> OpenSession {
    let meta = SessionMeta {
        id: id.to_owned(),
        path: Path::new(id).to_path_buf(),
        modified: std::time::SystemTime::UNIX_EPOCH,
        title: None,
    };
    OpenSession::new(meta, data, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::parse_jsonl;
    use std::time::SystemTime;

    fn meta(id: &str) -> SessionMeta {
        SessionMeta {
            id: id.to_owned(),
            path: Path::new(id).to_path_buf(),
            modified: SystemTime::UNIX_EPOCH,
            title: None,
        }
    }

    #[test]
    fn list_navigation_is_clamped() {
        let mut app = App::new(vec![meta("a"), meta("b")]);
        app.handle(Action::Up); // 0 で頭打ち
        assert_eq!(app.list_selected, 0);
        app.handle(Action::Down);
        assert_eq!(app.list_selected, 1);
        app.handle(Action::Down); // 末尾で頭打ち
        assert_eq!(app.list_selected, 1);
        app.handle(Action::Top);
        assert_eq!(app.list_selected, 0);
        app.handle(Action::Bottom);
        assert_eq!(app.list_selected, 1);
    }

    #[test]
    fn quit_from_list() {
        let mut app = App::new(vec![meta("a")]);
        app.handle(Action::Quit);
        assert!(app.should_quit);
    }

    #[test]
    fn back_from_timeline_returns_to_list() {
        let mut app = App::new(vec![]);
        app.screen = Screen::Timeline;
        app.open = Some(open_session_for_test("s", parse_jsonl("")));
        app.handle(Action::Back);
        assert_eq!(app.screen, Screen::SessionList);
        assert!(app.open.is_none());
    }

    #[test]
    fn toggle_branches_only_in_timeline() {
        let mut app = App::new(vec![]);
        app.handle(Action::ToggleBranches); // 一覧では無効
        assert!(!app.show_branches);
        app.screen = Screen::Timeline;
        app.open = Some(open_session_for_test("s", parse_jsonl("")));
        app.handle(Action::ToggleBranches);
        assert!(app.show_branches);
    }

    /// 単一行ブロックを 3 つ持つセッション (各 user 発話が 1 ブロック)。
    fn three_block_session() -> Session {
        parse_jsonl(
            "{\"type\":\"user\",\"message\":{\"content\":\"one\"}}\n\
             {\"type\":\"user\",\"message\":{\"content\":\"two\"}}\n\
             {\"type\":\"user\",\"message\":{\"content\":\"three\"}}",
        )
    }

    #[test]
    fn timeline_selection_is_clamped() {
        let mut app = App::new(vec![]);
        app.screen = Screen::Timeline;
        app.open = Some(open_session_for_test("s", three_block_session()));
        assert_eq!(app.open.as_ref().unwrap().selected, 0);
        app.handle(Action::Up); // 0 で頭打ち
        assert_eq!(app.open.as_ref().unwrap().selected, 0);
        app.handle(Action::Down);
        assert_eq!(app.open.as_ref().unwrap().selected, 1);
        app.handle(Action::Bottom);
        assert_eq!(app.open.as_ref().unwrap().selected, 2);
        app.handle(Action::Down); // 末尾で頭打ち
        assert_eq!(app.open.as_ref().unwrap().selected, 2);
        app.handle(Action::Top);
        assert_eq!(app.open.as_ref().unwrap().selected, 0);
    }

    #[test]
    fn enter_toggles_fold_of_foldable_block_only() {
        // block0: 複数行 (折りたたみ対象)、block1: 単一行 (非対象)。
        let jsonl = "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"a\\nb\"}]}}\n\
                     {\"type\":\"user\",\"message\":{\"content\":\"single\"}}";
        let mut app = App::new(vec![]);
        app.screen = Screen::Timeline;
        app.open = Some(open_session_for_test("s", parse_jsonl(jsonl)));
        assert!(!app.open.as_ref().unwrap().expanded[0]);
        app.handle(Action::Enter); // block0 を展開
        assert!(app.open.as_ref().unwrap().expanded[0]);
        app.handle(Action::Enter); // 折りたたみに戻す
        assert!(!app.open.as_ref().unwrap().expanded[0]);
        // 単一行ブロックは Enter で状態が変わらない。
        app.handle(Action::Down);
        app.handle(Action::Enter);
        assert!(!app.open.as_ref().unwrap().expanded[1]);
    }

    #[test]
    fn next_and_prev_hook_jump_to_hook_blocks() {
        // block0: user, block1: hook, block2: assistant, block3: hook。
        let jsonl = "{\"type\":\"user\",\"message\":{\"content\":\"a\"}}\n\
                     {\"type\":\"system\",\"subtype\":\"stop_hook_summary\",\"hookCount\":1}\n\
                     {\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"b\"}]}}\n\
                     {\"type\":\"system\",\"subtype\":\"stop_hook_summary\",\"hookCount\":1}";
        let mut app = App::new(vec![]);
        app.screen = Screen::Timeline;
        app.open = Some(open_session_for_test("s", parse_jsonl(jsonl)));
        assert_eq!(app.open.as_ref().unwrap().selected, 0);
        app.handle(Action::NextHook); // 0 -> 1 (最初の hook)
        assert_eq!(app.open.as_ref().unwrap().selected, 1);
        app.handle(Action::NextHook); // 1 -> 3 (次の hook)
        assert_eq!(app.open.as_ref().unwrap().selected, 3);
        app.handle(Action::NextHook); // 3 -> 1 (末尾から先頭へ巻き戻る)
        assert_eq!(app.open.as_ref().unwrap().selected, 1);
        app.handle(Action::PrevHook); // 1 -> 3 (先頭から末尾へ巻き戻る)
        assert_eq!(app.open.as_ref().unwrap().selected, 3);
    }

    #[test]
    fn hook_jump_is_noop_without_hooks() {
        let mut app = App::new(vec![]);
        app.screen = Screen::Timeline;
        app.open = Some(open_session_for_test("s", three_block_session()));
        app.handle(Action::NextHook);
        assert_eq!(app.open.as_ref().unwrap().selected, 0);
        app.handle(Action::PrevHook);
        assert_eq!(app.open.as_ref().unwrap().selected, 0);
    }

    #[test]
    fn toggle_branches_rebuilds_blocks() {
        let jsonl = r#"{"type":"user","isSidechain":true,"message":{"content":"side"}}"#;
        let mut app = App::new(vec![]);
        app.screen = Screen::Timeline;
        app.open = Some(open_session_for_test("s", parse_jsonl(jsonl)));
        // 既定 (分岐 OFF) ではサイドチェーンはブロックに含まれない。
        assert!(app.open.as_ref().unwrap().blocks.is_empty());
        app.handle(Action::ToggleBranches);
        // ON にすると再構築されてブロックが現れる。
        assert!(!app.open.as_ref().unwrap().blocks.is_empty());
    }
}
