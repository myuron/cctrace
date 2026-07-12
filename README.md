# cctrace

Claude Code のセッション記録 (JSONL) を人が読みやすい形で表示し、
**Skill / Hook が意図したタイミングで起動しているか**を確認するための
TUI ビューアです。読み取り専用で、外部送信は行いません。

## 使い方

```console
$ nix build
$ cd <確認したいプロジェクトの作業ディレクトリ>
$ /path/to/cctrace/result/bin/cctrace
```

起動した作業ディレクトリ (cwd) に対応する
`~/.claude/projects/<encoded-cwd>/*.jsonl` を列挙し、
選択したセッションをタイムライン表示します。

### キー操作

| キー             | 動作                            |
| ---------------- | ------------------------------- |
| `↑`/`↓`, `j`/`k` | カーソル移動                    |
| `Enter`          | セッションを開く / ブロック開閉 |
| `g` / `G`        | 先頭 / 末尾へ                   |
| `h` / `H`        | 次 / 前のフック起動へジャンプ   |
| `b`              | 分岐・サイドチェーン表示の切替  |
| `Esc`            | 一覧へ戻る                      |
| `q`              | 終了                            |

### タイムラインの見方

- `⚡` … フック起動。hookName・コマンド・実行時間・成否を表示
- `✦ Skill: <name>` … Skill 起動
- `[skill:<name>]` … その操作が Skill 由来であることを示すバッジ
- ヘッダの `hooks: N` … セッション内のフック起動記録数 (0 なら記録なし)

## Hook を cctrace に表示させるには (settings.json の設定)

フックの可視化には `.claude/settings.json` の設定が 2 段階で関わります。

### 1. 前提: 正しいスキーマで登録する

スキーマが誤っているとフックは**一度も発火せず、transcript にも何も残りません**
(エラーも出ず無言で無視されます)。正しい形式は以下の通りです。

```json
{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Write|Edit",
        "hooks": [
          { "type": "command", "command": "nix fmt 2>/dev/null || true" }
        ]
      }
    ]
  }
}
```

よくある間違い (どちらも無言で無視される):

- `"PostToolUse": "Write|Edit"` のように matcher を文字列で直接指定する
- `hooks` 配列をイベント名と同じ階層に置く

### 2. 自動で記録されるもの・されないもの

正しく登録されたフックが発火すると、追加設定なしで以下が記録されます。

| ケース                                 | transcript への記録              | cctrace 表示 |
| -------------------------------------- | -------------------------------- | ------------ |
| Stop フックの実行                      | `stop_hook_summary` (常に)       | ⚡ 表示      |
| PostToolUse 等がファイルを**変更した** | `hook_additional_context` (自動) | ⚡ 表示      |
| フックが**出力もファイル変更もしない** | **記録なし**                     | 見えない     |

例えばフォーマッタの場合、「整形の必要があった実行」だけが見え、
「既に整形済みで何もしなかった実行」は見えません。

### 3. すべての実行を表示したい場合

フックが stdout に JSON を出力すると、`hook_success`
(コマンド・実行時間・exitCode 付き) が**毎回**記録されます。
コマンド末尾に `systemMessage` の echo を足すだけです。

```json
{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Write|Edit",
        "hooks": [
          {
            "type": "command",
            "command": "nix fmt 2>/dev/null || true; echo '{\"systemMessage\":\"nix fmt hook fired\"}'"
          }
        ]
      }
    ]
  }
}
```

cctrace のタイムラインには毎回次のように表示されます。

```
⚡ PostToolUse:Write  nix fmt 2>/dev/null || true; echo…  263ms  ✓ ok
```

**トレードオフ**: Claude Code の UI にも毎回この 1 行が表示されます。
ノイズと感じる場合は echo を外してください (無変更の実行は見えなくなります)。

なお `hookSpecificOutput.additionalContext` でも痕跡は残せますが、
毎回モデルの文脈に注入されコンテキストを消費するため、
可視化目的では `systemMessage` を推奨します。

### 検証済みの範囲

実データで確認済みなのは **Stop** と **PostToolUse** の 2 イベントです
(Claude Code v2.1.204)。PreToolUse / UserPromptSubmit 等も同じ
`hook_*` attachment 形式で記録される可能性が高いですが、未検証です。

## 開発

```console
$ nix build          # ビルド
$ nix fmt            # フォーマット
$ nix run .#lint     # リント
$ nix run .#test     # テスト
```
