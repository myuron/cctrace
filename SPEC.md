# cctrace 仕様書 (SPEC)

> ステータス: ドラフト / 検討中
> 最終更新: 2026-07-10

本書は cctrace の仕様を定める。実装より先に「何を作るか」を確定させ、
勝手な前提で進めないための合意文書とする。未確定事項は末尾に明示する。

## 1. 目的

Claude Code のセッション記録を人が読みやすい形で表示し、
Hook や Skill が意図したタイミングで起動しているかを確認できるようにする。

短期的に動くだけのコードではなく、長期的に保守できる実装を優先する。

## 2. スコープ

今回の合意で確定した方針:

- **入力モード**: 過去セッション閲覧（静的ビューア）。実行中ファイルの監視・追従は**行わない**。
- **表示の焦点**: 会話タイムライン主体。その中で **Skill 起動**をハイライトする。
- **対象範囲**: cctrace を起動した作業ディレクトリ (cwd) に対応するプロジェクトのセッションのみ。
- **Skill 可視化に加え、構造的に検出可能な Hook を可視化する。**
  transcript に構造化フィールドが現れる Hook 記録のみを対象とする（§5.2）。
  Stop フック（`stop_hook_summary`）と、`hook_*` attachment として記録される
  PostToolUse 等の実行記録の両方を含む。

### 非対象 (Non-Goals)

- **痕跡を残さないフック実行の推測表示**（出力もファイル変更もしない実行は
  transcript に記録されず、検出不可能。§5.2）
- 実行中セッションのリアルタイム追従（tail 監視）
- 複数プロジェクト横断のセッション閲覧
- セッションの編集・削除・再生などの書き込み操作
- ネットワーク通信・外部送信（ローカルファイルのみを読む）

## 3. 用語

| 用語         | 定義                                                               |
| ------------ | ------------------------------------------------------------------ |
| セッション   | 1回の Claude Code 対話。1つの JSONL ファイルに対応する。           |
| エントリ     | JSONL の1行。1つの JSON オブジェクト。`type` を持つ。              |
| プロジェクト | cwd をエンコードした `~/.claude/projects/<encoded>` ディレクトリ。 |
| タイムライン | エントリを時系列に並べた表示。                                     |

## 4. 入力データ仕様

### 4.1 ファイルの場所

```
~/.claude/projects/<cwd をエンコードした名前>/<session-id>.jsonl
```

- cwd のエンコード規則は Claude Code の実装に従う（例: `/home/myuron/src/...` →
  `-home-myuron-src-...`）。cctrace は起動時の cwd から対象ディレクトリを特定する。
- 1ファイル = 1セッション。ファイルは追記型の JSONL（1行1 JSON）。

### 4.2 エントリ構造（実データで確認済み）

各行は `type` を持つ。確認された `type`:

- `user` / `assistant`: メッセージ本体。`message.content[]` は
  `text` / `thinking` / `tool_use` のいずれか。
- `system`: `subtype`（`local_command` / `turn_duration` 等）を持つ補助情報。
- `attachment`: 注入された付随情報。`attachment.type` は
  `skill_listing` / `task_reminder` / `command_permissions` /
  `agent_listing_delta` / `deferred_tools_delta` / `file` / `edited_text_file` /
  `already_read_file` 等。
- `file-history-snapshot` / `ai-title` / `last-prompt` / `mode` / `pr-link`:
  メタ情報。MVP では原則非表示（将来拡張）。

共通フィールド（存在するもの）:

- `uuid`, `parentUuid`: エントリは `parentUuid` で連結される**木構造**。
  `parentUuid == null` が起点。分岐（やり直し等）がありうる。
- `timestamp`: ISO8601 (例 `2026-07-10T16:52:00.569Z`)。
- `sessionId`, `cwd`, `gitBranch`, `permissionMode`, `isSidechain`,
  `attributionSkill` など。

### 4.3 堅牢性の前提

- 壊れた行・未知の `type`・未知のフィールドが存在しうる。
  **パースに失敗した行はスキップし、既知フィールドのみ解釈する**（前方互換）。
- ファイルはツール実行中に追記されうるが、MVP は起動時スナップショットを読む。

## 5. Hook / Skill 検出（本ツールの核）

### 5.1 Skill 起動 — 構造的に検出可能（確実）

- Skill 起動は `assistant` の `tool_use`（`name == "Skill"`, `input.skill` に名前）
  として現れる。
- Skill が駆動した後続のツール呼び出しには `attributionSkill: "<skill 名>"` が付く。
  → 「どの Skill がどの操作を駆動したか」を確実に追跡・可視化できる。
- 利用可能な Skill 一覧は `attachment.type == "skill_listing"` に載る。

### 5.2 Hook 起動 — 2 系統の構造化記録を検出する

実データ検証（Claude Code v2.1.204）の結果、フック実行は transcript に
**2 系統の構造化された記録**を残すことが判明した。cctrace は両方を専用の
タイムライン種別 (`EntryKind::Hook`) として parse し、⚡ 付きで強調表示する。

**(a) Stop フック**: `type == "system"` / `subtype == "stop_hook_summary"`

- `hookCount`: 実行されたフック数
- `hookInfos[]`: 各フックの情報（`durationMs` 等）
- `hookErrors[]`: フックが報告したエラー（空なら正常）
- `preventedContinuation`: フックが継続を阻止したか
- `level` / `hasOutput` / `toolUseID` など

**(b) PostToolUse 等のイベントフック**: `type == "attachment"` /
`attachment.type == "hook_*"`

- `hook_success`: フックが stdout に出力した場合に記録される。
  `hookName`（例 `PostToolUse:Write`）/ `hookEvent` / `toolUseID` /
  `exitCode` / `durationMs` / `stdout` / `stderr` / `command` を持つ。
- `hook_system_message`: フック出力 JSON の `systemMessage` の内容。
- `hook_additional_context`: フックが注入した context。**フックがファイルを
  変更した場合にも Claude Code が自動記録する**（例:「PostToolUse hook
  modified <path>」）。

**重要な制約**: フックが**出力もファイル変更もしない実行は transcript に
一切記録されない**。すべての実行を可視化したい場合は、フック側で
`{"systemMessage": "..."}` 等を stdout に出力する運用にする（本リポジトリの
`nix fmt` フックはこの運用を採る）。

**経緯の注記**: 当初「PostToolUse は痕跡を残さない」と結論していたが、これは
調査時点で hooks 設定のスキーマ誤りによりフックが一度も発火していなかった
ことによる誤り。設定修正後の実データで上記 (b) の記録を確認し訂正した。

## 6. 機能要件

### 6.1 セッション選択

- 起動時、cwd に対応するプロジェクトディレクトリ配下の `*.jsonl` を列挙する。
- 一覧を**更新日時の新しい順**で表示し、キー操作で選択・決定する。
- 各項目に表示する情報（案・要確認）: 更新日時 / 先頭プロンプト要約 (`last-prompt` 等) /
  エントリ数 / ファイル名(session-id)。
- 対象ディレクトリが存在しない / JSONL が0件の場合は、その旨を明示して終了しない
  （空状態表示）。

### 6.2 タイムライン表示

- 選択したセッションのエントリを**時系列（timestamp 昇順）**に並べて表示する。
- 各エントリの描画ルール（§7）に従い、役割ごとに視覚的に区別する。
- Skill 起動、`attributionSkill` の付いた操作をハイライトする。
- 上下スクロール・セッション一覧への復帰ができる。

### 6.3 操作（キーバインド・案 / 要確認）

| キー               | 動作                                   |
| ------------------ | -------------------------------------- |
| `↑`/`↓` or `j`/`k` | カーソル移動                           |
| `Enter`            | セッションを開く                       |
| `Esc` / `q`        | 一覧へ戻る / 終了                      |
| `g`/`G`            | 先頭 / 末尾へ                          |
| `b`（案）          | 分岐・サイドチェーン表示の ON/OFF 切替 |

## 7. エントリ描画ルール（MVP）

| type / 内容                       | 表示                                                                             |
| --------------------------------- | -------------------------------------------------------------------------------- |
| `user` (text)                     | ユーザー発話として表示                                                           |
| `assistant` text                  | アシスタント応答                                                                 |
| `assistant` thinking              | 思考。折りたたみ or 淡色で区別（案）                                             |
| `assistant` tool_use              | ツール名 + 主要 input を1行要約。`Skill` は強調                                  |
| `attributionSkill` 付き           | 「Skill 由来」バッジを付与                                                       |
| `attachment` (hook\_\* )          | フック実行として ⚡ で強調。hookName・コマンド・実行時間・エラー・注入内容を表示 |
| `attachment` (skill_listing 等)   | 種別を短く表示（詳細は折りたたみ、案）                                           |
| `system` (hook summary)           | フック実行として ⚡ で強調。種別・実行数・実行時間・エラー・継続阻止を表示       |
| `system` (その他)                 | 補助情報として淡色表示（`turn_duration` 等は集約 or 非表示、案）                 |
| メタ系 (ai-title/mode/pr-link 等) | MVP 非表示（将来拡張）                                                           |

木構造 (`parentUuid`) の扱い:

- **既定は主系列を線形化して表示**する（timestamp 昇順で並べる）。
- **分岐（やり直し等の枝）とサイドチェーン (`isSidechain`) は、キー操作で
  表示 ON/OFF を切り替えられるオプション**とする。既定は OFF（線形のみ）。
- ON のときの見せ方（インデント / バッジ / 別ペイン等）は実装時にプロトタイプで詰める。

## 8. UI レイアウト（Ratatui・案）

- 画面遷移: 「セッション一覧」→「タイムライン」の2画面。
- タイムライン画面案: 上部にセッション情報（cwd / branch / session-id）、
  中央にスクロール可能なタイムライン、下部にキーヒント。
- 具体的なペイン構成・配色は実装時にプロトタイプで詰める。

## 9. 非機能要件

- **堅牢性**: 壊れた行・未知フィールドで落ちない（§4.3）。
- **パフォーマンス**: 数百 KB〜数 MB の JSONL を実用的な速度で開ける。
  大きなファイルでも UI がブロックしすぎないこと（目標値は要合意）。
- **セキュリティ**: 読み取り専用。ホームディレクトリ配下の該当ファイルのみ読む。
  外部送信しない。パス探索は cwd 由来のディレクトリに限定する。
- **保守性**: 過剰な抽象化を避け、明快な構造を優先する。

## 10. 未確定事項 / 要検討 (Open Questions)

**決定済み**:

- フックの構造化記録（`stop_hook_summary` と `hook_*` attachment）は可視化対象
  （§5.2）。出力もファイル変更もしない実行は記録が残らないため検出対象外
  （§2 / §5.2）。
- 木構造は既定で線形化。分岐・サイドチェーンは ON/OFF 切替オプション（§7）。

残りの検討事項:

1. `thinking` / `attachment` / `system` を既定で表示するか折りたたむか。
2. セッション一覧に出すメタ情報の項目と、要約の作り方。
3. パフォーマンス目標値（対象とする最大ファイルサイズ・許容起動時間）。
4. キーバインドの最終確定。

## 11. 受け入れ基準（テスト観点・TDD 向け）

- 代表的な JSONL（text/thinking/tool_use/Skill/attributionSkill を含む）を
  与えると、期待するタイムライン構造にパースできる。
- 壊れた行・未知 type を含む JSONL を与えても panic せず、既知行のみ描画できる。
- Skill 起動と `attributionSkill` 由来の操作が識別・ハイライトできる。
- 対象ディレクトリ不在 / JSONL 0件で、空状態を返し正常に扱える。
