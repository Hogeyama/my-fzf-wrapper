日本語で簡潔かつ丁寧に回答してください。

# リポジトリガイドライン

## プロジェクト構成とモジュール整理

* **`src/`**: Rust ソースコード。
  * **Core**: `main.rs` (エントリポイント), `lib.rs`, `client.rs` (CLIクライアント), `server.rs` (サーバー), `config.rs` (設定), `logger.rs` (ログ), `state.rs` (状態管理), `method.rs` (JSON-RPC メソッド定義)。
  * **Neovim連携**: `nvim.rs` (Neovim との通信・制御)。
  * **Modes**: `src/mode/` 配下に各 fzf モードを定義。`mod.rs` で全モードを登録。
  * **Utils**: `src/utils/` (fzf操作ラッパーなど)。
* **ツール類**:
  * `Cargo.toml`: パッケージ定義 (crate name: `fzfw`)。
  * `justfile`: タスクランナー (`just build`, `just server` 等)。
  * `flake.nix`: Nix による開発環境・ビルド定義。
  * `.envrc` / `.envrc.local`: direnv 設定。
* **テスト**: `tests/` (統合テスト)。

## アーキテクチャ概要

* **Client-Server 構成**:
  * CLI (`client.rs`) はサーバー (`server.rs`) に JSON-RPC でリクエストを送り、fzf の初期化・制御を行います。
  * Server は各モード (`ModeDef` 実装) をホストし、fzf からのイベント (Load, Preview, Execute) を処理します。
* **Neovim 連携**:
  * `nvim.rs` を通じて Neovim インスタンスと通信し、バッファ操作やファイルオープンなどを行います。

## モード実装ガイド (`src/mode/*.rs`)

新しいモードを追加する場合は、`src/mode/` にファイルを作成し、`ModeDef` トレイトを実装してください。

### `ModeDef` トレイト

```rust
pub trait ModeDef {
    fn name(&self) -> &'static str;
    fn load<'a>(&'a self, config: &'a Config, state: &'a mut State, query: String, item: String) -> LoadStream<'a>;
    fn preview<'a>(&'a self, config: &'a Config, win: &'a PreviewWindow, item: String) -> BoxFuture<'a, Result<PreviewResp>>;
    fn execute<'a>(&'a self, config: &'a Config, state: &'a mut State, item: String, args: serde_json::Value) -> BoxFuture<'a, Result<()>>;
    fn fzf_bindings(&self) -> (fzf::Bindings, CallbackMap);
}
```

* **`load`**: fzf に表示する候補一覧をストリーム (`LoadStream`) で返します。非同期に逐次生成可能です。
* **`preview`**: 選択中のアイテムに対するプレビュー内容 (`PreviewResp`) を返します。
* **`execute`**: アイテム選択時のアクションを定義します (任意の引数を受け取り可能)。
* **`fzf_bindings`**: fzf のキーバインドと、それに対応するコールバック関数を定義します。`src/mode/config_builder.rs` のマクロ (`bindings!`) を使うと便利です。

### 実装のポイント

* **1ファイル1モード**: 原則として `src/mode/` 直下に各モードのファイルを置きます。
* **公開**: `src/mode/mod.rs` の `all_modes()` に追加して有効化します。

## テストガイドライン

### ユニットテスト (`src/**/*.rs`)

* ロジック単体のテストは各モジュール内に `#[cfg(test)]` で記述します。

### 統合テスト (`tests/*.rs`)

* `tests/common/mod.rs` の `TestHarness` を使用して、実際のプロセス間通信を模倣したテストを行います。
* **`TestHarness::spawn()`**: テスト用のサーバー環境を立ち上げます。
* **`h.load("mode_name", ...)`**: 特定モードのロード動作を検証し、出力 (stdout) をアサートします。
* 外部依存 (Neovim 等) は可能な限りモックするか、テスト環境で再現可能な範囲に留めます。

## ビルド・開発コマンド

* **開発環境セットアップ**: `direnv allow` (推奨) または `nix develop`。
* **ビルド**: `just build` (または `cargo build`)。
* **実行**: `just server` (または `cargo run --`)。
* **整形 & Lint**: `cargo fmt`, `cargo clippy`。

## 主なモード一覧

* `fd`: ファイル検索 (外部コマンド `fd` 利用)。
* `livegrep`: ripgrep による動的検索。
* `buffer`: Neovim バッファ一覧。
* `mru`: 最近使ったファイル (MRU)。
* `git_diff`, `git_status`, `git_log`: Git 連携。
* `browser_history`, `browser_bookmark`: ブラウザ連携。
