//! cctrace: Claude Code のセッション記録を読みやすく表示し、Skill 起動を可視化する
//! 静的ビューア (SPEC §1)。読み取り専用で cwd 由来のプロジェクトのみを対象とする。

mod app;
mod model;
mod project;
mod session;
mod ui;

use std::io;

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};

use app::{Action, App};

fn main() -> io::Result<()> {
    let cwd = std::env::current_dir()?;
    let Some(home) = project::claude_home() else {
        eprintln!("HOME 環境変数が設定されていないため、~/.claude を特定できません。");
        std::process::exit(1);
    };

    let sessions = project::discover_sessions(&home, &cwd);
    let app = App::new(sessions);

    let mut terminal = ratatui::init();
    let result = run(&mut terminal, app);
    ratatui::restore();
    result
}

fn run(terminal: &mut ratatui::DefaultTerminal, mut app: App) -> io::Result<()> {
    while !app.should_quit {
        terminal.draw(|frame| ui::draw(frame, &mut app))?;

        // 静的ビューアなのでキー入力があるまでブロックしてよい (ポーリング不要)。
        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            if let Some(action) = map_key(key.code, key.modifiers) {
                app.handle(action);
            }
        }
    }
    Ok(())
}

/// crossterm のキーイベントを抽象 `Action` に写像する (SPEC §6.3)。
fn map_key(code: KeyCode, modifiers: KeyModifiers) -> Option<Action> {
    // Ctrl-C は常に終了。
    if modifiers.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('c') {
        return Some(Action::Quit);
    }
    match code {
        KeyCode::Up | KeyCode::Char('k') => Some(Action::Up),
        KeyCode::Down | KeyCode::Char('j') => Some(Action::Down),
        KeyCode::Char('g') => Some(Action::Top),
        KeyCode::Char('G') => Some(Action::Bottom),
        KeyCode::Enter => Some(Action::Enter),
        KeyCode::Esc => Some(Action::Back),
        KeyCode::Char('q') => Some(Action::Quit),
        KeyCode::Char('b') => Some(Action::ToggleBranches),
        KeyCode::Char('h') => Some(Action::NextHook),
        KeyCode::Char('H') => Some(Action::PrevHook),
        _ => None,
    }
}
