---
name: commit
description: commitする際の規約。commitメッセージを書く際に使用します。
---

# commit

## 概要

このSkillはコミットメッセージのための規約です。明示的なコミット履歴を作成するためのルールを提供します。
コミットメッセージで機能追加、修正、破壊的変更などを説明することで、SemVerと協調動作します。

## 目的

- 変更履歴を自動的に生成するため
- コミットの型に基づき、SevVer単位で自動的に履歴をまとめるため
- 他の変更者に変更の内容を簡単に伝えるため
- ビルドや公開の処理をトリガーするため
- より構造化されたコミット履歴を調査できるようにすることで、他の変更者がプロジェクトに貢献しやすくするため

## テンプレート

```
<type>[optional scope]: <description>

[optional body]

[optional footer(s)]
```

## example

### タイトルおよび破壊的変更のフッターを持つコミットメッセージ

```
feat!: allow provided config object to extend other configs

BREAKING CHANGE: `extends` key in config file is now used for extending other config files
```

### 本文を持たないコミットメッセージ

```
docs: correct spelling of CHANGELOG
```

### スコープを持つコミットメッセージ

```
feat(lang): add polish language
```

### 複数段落からなる本文と複数のフッターを持ったコミットメッセージ

```
fix: prevent racing of requests

Introduce a request id and a reference to latest request. Dismiss
incoming responses other than from latest request.

Remove timeouts which were used to mitigate the racing issue but are
obsolete now.

Reviewd-by: Z
Refs: #123
```

## 仕様

- コミットは`feat`や`fix`などの型から始めなければならない
- 破壊的変更がある場合は、型の後ろに破壊的変更を示す`!`を付与しなければならない
- 影響範囲が明確な場合は、型の後ろにスコープを示す`()`の中にコードベースのセクションである名詞を記載し、付与しなければならない。例:`fix(parser):`
- コミットが新しい機能を追加するときは、型`feat`を使わなければならない
- コミットがバグ修正を行うときは、型`fix`を使わなければならない
- 型/スコープの後ろのコロンとスペースの直後にタイトルが続かなければならない
- タイトルはコード変更の短い要約である必要がある
- 追加の情報がある場合、タイトルより長い本文を追加しなければならない。特にレビュアーに注視するべきものがある場合は積極的に追加しなければならない
- 本文は改行で区切られた複数の段落で構成しなければならない
- 破壊的変更がある場合は、footer`BREAKING CHANGE`を付与し、後ろにコロンとスペース、そして破壊的変更の短い要約を続けて記載しなければならない。
- 型`feat`や`fix`以外に当てはまる場合は、以下の型を適切に選択する必要がある
- 情報の単位は、`BREAKING CHANGE`を除いて、大文字と小文字を区別してはならない
- Commit messageに絵文字を使用してはいけない
