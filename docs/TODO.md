# TODO

## 課題

### all_modes() の手動登録

新モード追加時に `src/mode/mod.rs` の `pub mod xxx` と `all_modes()` の
`Box::pin(|| f(...))` の両方を手書きする必要がある。
`pub mod` は追加するのに `all_modes()` への登録を忘れると、
コンパイルは通るがモードが使えないサイレントバグになる。

### コールバック名が通し番号の文字列

`ConfigBuilder::gen_name` が `"callback1"`, `"callback2"`, ... と生成し、
fzf からの応答で文字列マッチで逆引きする。`// TODO use gensym` が既にある。

`default_bindings()` のコールバックと各モードのコールバックが同じカウンターを
共有しており (`callback_counter` を引き継ぎ)、現状は壊れないが構造として脆い。
`default_bindings()` 側の変更がモード側のカウンターオフセットを変えうる。

なお、現在は全モードのコールバックが起動時に `all_modes` に事前登録されるため、
モード間でコールバック名が衝突しても `current_mode_name` ベースの dispatch で
正しいモードの CallbackMap が参照される。ただし名前が意味を持たないのは変わらず。

## ワクワク: fzf新機能の活用

fzf 0.30頃に作り始めた制約から解放されるための改善案。

### ~~`transform` + `dispatch` による動的キーバインド~~ → execute-silent に統合済み

全キーバインドが `execute-silent` + `_key:` プレフィックス経由に統一。
サーバーハンドラは load/preview/execute の 3 つのみ。
`Dispatch` メソッド・`Transform` アクションは廃止済み。

### ~~`--listen` ソケットでサーバー→fzf直接通信~~ → 実装済み

`--listen` で fzf にアクションを POST する `FzfClient` を実装済み。
モード切替・キー dispatch の両方で使用中。
fzf プロセスの再起動なしでモード切替が可能になった。

### サーバー起点の自動リロード (工数: 中)

`fzf_client.post_action("reload(...)")` でサーバー側からリロードを発火できる。
ファイル監視 (inotify/fswatch) やタイマーと組み合わせて:

- **git_status / git_diff**: ファイル保存時に自動リロード。
  Neovim の `BufWritePost` イベント → サーバーに通知 → fzf に reload POST。
  手動 ctrl-r が不要になる。
- **process_compose**: プロセス状態のポーリングリロード。
  一定間隔で `post_action("reload(...)")` を送ればリアルタイム監視になる。
- **diagnostics**: LSP の diagnostics 更新時に自動リロード。

### コールバック内からの fzf フィードバック (工数: 小)

execute コールバック内で `fzf_client.post_action("change-header(...)")` を使い、
操作結果をユーザーに即座にフィードバックできる。

- git_diff の stage/unstage 後: `change-header(Staged: 3 files)` で状態表示
- git_branch の push 後: `change-header(Pushed to origin/main)` で結果通知
- 重い操作中: `change-header(Processing...)` → 完了後に `change-header(Done!)`

execute-silent 中でも POST はキューされるので、完了直後に表示が更新される。

### Neovim との双方向連携 (工数: 中)

サーバーは Neovim と fzf の両方に接続している。
Neovim 側のイベントを fzf の表示に反映できる:

- バッファ切替時に buffer モードのカーソル位置を追跡
  (`post_action("pos(N)")` で現在のバッファにジャンプ)
- Neovim の diagnostics 変更を即座に diagnostics モードに反映
- ファイル保存時に mru / visits の順序を動的に更新

### プログレス表示付きストリーミング (工数: 中)

重いロード中に `post_action("change-header(Loading... 42%)")` で進捗を表示。
現在のストリーミング load と組み合わせれば:

1. load コールバックがアイテムを逐次 yield
2. 並行して `fzf_client.post_action("change-header(N items loaded)")` で件数更新
3. 完了時に `change-header([/path/to/dir])` で通常ヘッダに戻す

livegrep の検索結果件数やgit log の読み込み中表示に使える。

### `--footer` でキーバインドヘルプ表示 (工数: 小)

fzf 0.63 の `--footer` と `transform-footer` で、モードごとのキーバインドヘルプを
下部に表示。モード切替時に動的更新可能。
例: `ctrl-y:yank | enter:actions | ctrl-l:branches | pgdn:menu`

### `--id-nth` でリロード時の選択状態保持 (工数: 小)

fzf 0.71 の `--id-nth` で `reload` 時にアイテムの同一性をフィールドで追跡し、
選択状態とカーソル位置を保持。git_status や git_diff でリロード後にカーソルが
飛ばなくなる。

### `bg-transform-*` で重いプレビュー情報の非同期表示 (工数: 中)

fzf 0.63 の `bg-transform-header` / `bg-transform-preview-label` で、
重い情報（git blame の要約、ファイル統計など）をバックグラウンドで計算して
ヘッダやラベルに表示。UIをブロックしない。

### `change-nth` で検索対象フィールドの切替 (工数: 小)

fzf 0.58 の `change-nth` で、例えば `git_status` モードでファイル名だけで
検索するか、ステータス(M/A/D)込みで検索するかをキーバインドで切り替え可能。

### `exclude` でアイテムの動的除外 (工数: 小)

fzf 0.60 の `exclude` / `exclude-multi` で、git_diff や git_status で
処理済みのファイルを一時的にリストから除外。リロードで復元。

### `--style` でUI改善 (工数: 小)

fzf 0.58 の `--style full` でセクション別ボーダー。
`--ghost` (0.61) で空クエリ時のプレースホルダ表示も可能。
