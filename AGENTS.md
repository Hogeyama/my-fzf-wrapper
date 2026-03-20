# my-fzf-wrapper

## 構成

* **Core**: `lib.rs`, `client.rs` (CLI), `server.rs`, `config.rs`, `env.rs` (実行環境), `state.rs`, `method.rs` (JSON-RPC), `nvim.rs` (Neovim連携)。
* **Modes**: `src/mode/` 配下に1ファイル1モード。`mod.rs` の `all_modes()` で登録。
  * `src/mode/lib/`: 共通ユーティリティ (`actions.rs`, `item.rs`, `cache.rs`)。
* **Utils**: `src/utils/` (fzf, 外部コマンドラッパー等)。
* **テスト**: `tests/` (`TestHarness` による統合テスト)。

## アーキテクチャ

* Client-Server 構成。CLI が JSON-RPC でサーバーにリクエストし、fzf を制御。詳細は [./docs/architecture.md][]

## モード追加手順

1. `src/mode/` にファイル作成、`ModeDef` を実装。
2. `src/mode/mod.rs` の `all_modes()` に追加。

## ビルド・開発

* `cargo build`
* `cargo fmt`
* `cargo clippy --all-targets -- -D warnings`
* `cargo test`
