日本語で簡潔かつ丁寧に回答してください。

# リポジトリガイドライン

## プロジェクト構成とモジュール整理

* `src/` — Rust ソース。主要モジュール: `main.rs`, `lib.rs`, `client.rs`, `server.rs`, `nvim.rs`, `config.rs`, `logger.rs`、さらに `mode/`（fzf モード）と `utils/`。
* ツール類: `Cargo.toml`（crate `fzfw`）、`justfile`（タスクランナー）、`flake.nix`（Nix 開発/ビルド）、`.envrc` / `.envrc.local`（direnv）。
* 成果物: `target/`（ビルド）、`.direnv/`（開発シェル）。これらはコミット禁止。

## ビルド・テスト・開発コマンド

* 開発シェル: direnv 有効なら単にリポジトリへ `cd`（初回は `direnv allow`）。direnv 無効や CI 環境では `nix develop` を使用（ツールチェーンをインストールし、`FZF_MANPATH` などを設定）。
* Rust ビルド: `just build` または `cargo build`。
* ローカル実行: `just server` または `cargo run --`。
* Nix パッケージ化: `nix build .#fzfw`（ランタイム依存を含むラッパー）または `nix run .#fzfw`。
* 環境設定のヒント: `.envrc` で `FZFW_FD_EXCLUDE_PATHS` と `RUSTFLAGS=-Zlinker-features=-lld` をエクスポート。

## コーディングスタイルと命名規則

* Rust 2021。整形は `cargo fmt`、Lint は `cargo clippy -- -D warnings`。
* `src/mode/` 配下のモジュールは 1 ファイル 1 モードとし、`ModeDef` 実装を公開すること。
* 小さく合成可能な関数を推奨。ライブラリコード内でのパニックは避け、必要に応じて `anyhow::Result` を使用。

## テストガイドライン

- 方針概要
  - ユニットテストは各モジュール内で `#[cfg(test)]` を用いて追加し、統合テストは `tests/` に配置。
  - 実行は `cargo test`。
  - 重要経路（モードのロード/プレビューのコールバック、状態遷移、ユーティリティ）を優先的にカバー。
  - 外部プロセス・外部コマンドへの依存は避け、モック/フェイクを用いる。

- モード（例: `src/mode/fd.rs`）における具体例と推奨パターン
  - Load ストリームの検証
    - 一時ディレクトリ（`tempfile::TempDir`）に極小シェルスクリプトを生成し、`tokio::process::Command` で実行して擬似出力を作る。
    - Unix 環境では `std::os::unix::fs::PermissionsExt` で実行ビットを付与。
    - `futures::StreamExt::next` でストリームを順に読み、
      - 1つ目のレスポンスは `is_last == false` かつ `items` に期待行が入ること、
      - 続くレスポンスで `is_last == true`（終端）が来ること、
      - 以降は `None` となること、を確認。
    - チャンクサイズ（例: `chunks(100)`）など内部実装の細部には依存せず、順序と終端のセマンティクスを検証する。
  - Preview の検証
    - 依存性注入を用いる。`preview(item, |path| bat::render_file(path))` のような構造に対し、テストでは `|_| async { Ok("OK".into()) }` のようなフェイク renderer を渡す。
    - 返る `PreviewResp.message` に期待文字列が含まれることを確認。外部コマンド（bat 等）は呼ばない。
  - 非同期テスト
    - `#[tokio::test]` を使用し、`futures` のユーティリティ（`StreamExt` 等）で検証する。
  - クロスプラットフォーム注意点
    - 上記の実行ビット付与は Unix 前提。必要に応じて `#[cfg(unix)]` などでガードするか、CI 対象 OS に合わせる。

- エラーパスの取り扱い
  - 可能であれば成功系とエラー系を両方テストする。`utils::command::command_output_stream` は子プロセスの非ゼロ終了を即エラーにしない点に留意し、読み取りエラーなど実際に `Err` を返し得る箇所を対象にする。

- 依存関係
  - `tempfile` などは既に依存に含まれているためそのまま使用可。新規に必要なものがあれば dev-dependencies へ追加。

- カバレッジの目安（優先度順）
  - モードの `load` と `preview` の基本動作（成功系）。
  - モードのキーバインドに紐づくコールバックの副作用が外部呼び出しに依存しない範囲で検証可能ならユニットテスト化（難しい場合は統合テストで）。
  - ユーティリティ関数（パーサ、整形、状態遷移ヘルパ）。

## コミット & プルリクエストガイドライン

* Conventional Commits を使用: `feat:`, `fix:`, `chore:`, `refactor:`、スコープは任意（例: `fix(server): …`）。
* メッセージは簡潔に。英語要約を推奨（詳細は日本語でも可）。
* PR には: 明確な説明、関連 Issue、再現手順やデモ手順、TUI 動作のスクリーンショットや asciicast を含めると有用。
* PR は小さく焦点を絞ること。外部ツール/バージョンへの影響があれば明記。

## セキュリティ & 設定のヒント

* 秘密情報はコミット禁止。ローカル上書きは `.envrc.local` を使用。
* 実行時は CLI ツール群（fzf, fd, ripgrep, bat など）に依存。Nix 使用時はラッパー `fzfw` が `PATH` を適切に設定。
* 開発シェルへの自動エントリには direnv 推奨。それ以外では `nix develop` が一貫したツールチェーンを保証。

## アーキテクチャ概要

* CLI がサーバー（`client.rs` / `server.rs`）と通信し fzf を駆動。`src/mode/` のモードが動作とキーバインドを定義。Neovim 連携は `nvim.rs` に実装。
