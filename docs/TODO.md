# TODO

## 課題

### all_modes() の手動登録

新モード追加時に `src/mode/mod.rs` の `pub mod xxx` と `all_modes()` の
`Box::pin(|| f(...))` の両方を手書きする必要がある。
`pub mod` は追加するのに `all_modes()` への登録を忘れると、
コンパイルは通るがモードが使えないサイレントバグになる。

## ワクワク: fzf新機能の活用

fzf 0.30頃に作り始めた制約から解放されるための改善案。

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

---

## ワクワク+: 複数機能を組み合わせた大型改善

上記の個別機能を組み合わせることで、ツールの性格を根本的に変える改善案。

### コンテキスト認識アクション — 二重 fzf の廃止 (工数: 中)

**使う機能**: `transform()` (0.45) + `--footer` / `change-footer` (0.63) + `change-header` (0.40) + `toggle-bind` (0.59)

現在の `select_and_execute!` は「アイテム選択 → もう一つ fzf を開いてアクション選択」
という二段階 UI。これを廃止し、**フォーカス中アイテムに応じて footer にアクション一覧を
動的表示**する。

- git-diff で staged hunk にカーソル → footer: `[s:unstage] [x:discard] [enter:open]`
- git-diff で untracked file にカーソル → footer: `[s:stage] [i:ignore] [enter:open]`
- `$FZF_SELECT_COUNT > 0` のとき → footer: `[s:stage selected (3)] [enter:open all]`
- 実行後: `change-header(Staged: 3 files)` で結果を即座にフィードバック
- `click-footer` (0.65) でフッターのアクションラベルをクリック可能に

`ModeDef` に `context_actions(item) -> Vec<ContextAction>` を追加。
`focus` イベント (0.37) でアイテム変更を検知し、サーバーが `change-footer` を POST。

### マルチセレクト一括操作 + ライブプレビュー (工数: 中)

**使う機能**: `change-multi` (0.51) + `multi` event (0.64) + `$FZF_SELECT_COUNT` (0.46) + `--accept-nth` (0.60)

multi-select を「複数選んで Enter」から「選択セットをキュレーションし、
結果をプレビューしてから実行」に昇格させる。

- git-diff: 5 つの hunk を選択 → preview に**結合パッチ**を表示 → 確認して一括 stage
- livegrep: 複数ファイル選択 → preview に sed/ripgrep の一括置換コマンドを生成・表示
- git-branch: 複数ブランチ選択 → preview にマージ/リベース計画を表示
- `multi` イベントで選択変更のたびに preview を更新
- `--info-command` (0.54) で `"3 selected | enter: stage all"` のようなカスタム info 表示

`ModeDef` に `batch_preview(items)` と `batch_execute(items)` を追加。

### ライブフォーカス連動: Neovim ↔ fzf (工数: 大)

**使う機能**: `GET /` state endpoint (0.43) + `focus` event (0.37) + Neovim RPC

fzf でアイテム間を**移動するだけで** Neovim が連動する。Enter 不要。

- livegrep: カーソル移動 → Neovim が該当ファイルの該当行にスクロール＋ハイライト
- git-diff: hunk 間を移動 → Neovim に inline diff overlay を表示
- pr-threads: スレッド移動 → 該当コード行に review comment を virtual text 表示
- diagnostics: 移動 → Neovim のカーソルが該当行にジャンプ

`focus` イベントでサーバーにアイテムを通知。サーバーは `GET /` で fzf の状態を取得し、
`ModeDef::on_focus(item) -> Option<NvimAction>` の結果を debounce (100ms) して
Neovim RPC で実行。IDE の検索パネルに匹敵する統合体験になる。

### 非同期バックグラウンド情報エンリッチ (工数: 大)

**使う機能**: `bg-transform-*` (0.63) + `--id-nth` (0.71) + `--info-command` (0.54) + `--freeze-left` (0.67) + `alt-bg` (0.62)

「高速ロード vs 豊富な情報」のトレードオフを解消。
アイテムを即座に表示し、メタデータをバックグラウンドで段階的に付加する。

- git-branch: ブランチ名を即表示 →
  バックグラウンドで CI status ✓/✗, PR title, last commit author を付加
- pr-list: PR タイトル即表示 →
  check status (pass/fail/pending), review approval count, conflict 有無を付加
- fd: ファイル名即表示 →
  git blame (last modifier), ファイルサイズ, 言語アイコンを付加
- `--id-nth` でエンリッチ後もカーソル位置・選択状態を保持
- `--freeze-left` で主要カラム (名前) を常に表示しつつ右側にメタデータを展開
- `alt-bg` でストライプ表示し、密な情報を読みやすく
- `--info-command` で `"Loading metadata... 42/100"` とエンリッチ進捗を表示

VS Code の GitLens のような progressive loading 体験。

### モード連鎖パイプライン (工数: 大)

**使う機能**: `--footer` (breadcrumb) + `trigger()` (0.65) + `--id-nth` (0.71) + `change-prompt` + `search()` (0.59)

モードの出力を次のモードの入力にする**視覚的シェルパイプライン**。

- livegrep → git-log: grep 結果を特定コミットの変更ファイルに絞り込み → open
- fd → diagnostics: 選んだファイルの diagnostics だけ表示
- git-branch → git-log: 特定ブランチのログだけ表示 → git-diff: そのコミットの差分
- footer に `livegrep > git-log(abc1234) > results` とパンくずリスト表示
- `alt-left` で前のステージに戻る（`--id-nth` で選択状態を復元）
- `search()` で前のステージのクエリを次のステージに引き継ぎ

`State` に `Vec<PipelineStage>` を追加。
`ModeDef` に `load_from_pipe(items, context)` を追加。
27 モードの **組合せ爆発** — N×N の探索パスが生まれる。

### Raw モードによるインタラクティブ git staging (工数: 中)

**使う機能**: `--raw` (0.66) + `exclude` (0.60) + `reload-sync` (0.36) + `--id-nth` (0.71) + `toggle-raw`

git-diff / git-status を **magit / lazygit 的な「全体を見ながら操作」** に変える。

- `--raw` で全アイテムを常に表示。非マッチは dim 表示 (`nomatch` カラー)
- stage 済みファイルも薄く残るので、全体の進捗が一目でわかる
- `exclude` で処理済みアイテムを一時的に除外 → `toggle-raw` で全体表示に戻す
- `reload-sync` + `--id-nth` で stage/unstage 後にカーソル位置を保持
- `up-match` / `down-match` (0.66) で未処理アイテム間を素早くジャンプ

現在の git-diff は「マッチするものだけ表示 → 操作 → reload」のサイクル。
Raw モードなら常に全体像が見え、操作のたびに「残り何個」が直感的にわかる。

### ダッシュボードモード — fzf を TUI モニターに (工数: 中)

**使う機能**: `--no-input` / `hide-input` (0.59) + `--footer` (0.63) + periodic `reload` + `--gap` (0.56) + multi-line (0.53) + `--wrap` (0.54) + `--highlight-line` (0.52)

fzf を fuzzy finder ではなく**リアルタイム TUI ダッシュボード**として使う。

- 入力欄なし (`--no-input`) のリードオンリー表示
- process-compose: プロセス状態をポーリング reload で更新。
  multi-line でログ末尾も inline 表示。`--gap` でプロセス間を視覚的に分離。
- CI dashboard: `gh run list` を定期 reload。
  pass/fail/pending をカラー付きで表示。`--highlight-line` で失敗 run を強調。
- PR review board: 自分がレビュアーの PR を一覧。
  approved/changes-requested/pending をリアルタイム更新。
- `show-input` / `toggle-input` でフィルタリングモードに一時切替可能
- footer にリフレッシュ間隔と最終更新時刻を表示

---

## 付録A: 現在使用中の fzf 機能一覧

ワクワクセクションの検討材料として、現在のアーキテクチャで使用中／未使用の fzf 機能を整理する。

### 使用中の fzf オプション

| オプション | 用途 |
|-----------|------|
| `--ansi` | ANSI カラー表示 |
| `--header-lines 1` | 先頭行をヘッダに |
| `--layout reverse` | 逆順レイアウト |
| `--preview [cmd]` | プレビューペイン |
| `--preview-window right:50%:noborder` | プレビュー位置 |
| `--prompt [str]` | モード別プロンプト |
| `--bind [key:action]` | キーバインド (モードから組立) |
| `--listen [socket]` | HTTP ソケット (FzfClient の POST 先) |
| `--no-sort` | ソート無効化 |
| `--multi` | マルチセレクト |
| `--query [str]` | 初期クエリ |

### 使用中の fzf アクション

| アクション | 用途 | 実装箇所 |
|-----------|------|---------|
| `reload[cmd]` | アイテムリスト再読み込み | 外部コマンド spawn |
| `execute[cmd]` | コマンド実行 (完了待ち) | server 経由 |
| `execute-silent[cmd]` | コマンド実行 (非同期) | 全キーバインドのディスパッチ |
| `change-prompt[text]` | プロンプト動的変更 | モード切替 |
| `toggle-sort` | ソート切替 | モード依存 |
| `enable-search` / `disable-search` | 検索の有効/無効 | モード依存 |
| `change-preview-window[spec]` | プレビュー窓のリサイズ | shift-right 等 |
| `deselect-all` | 選択解除 | モード切替時のクリーンアップ |
| `clear-query` | クエリクリア | モード切替時のクリーンアップ |
| `clear-screen` | 画面リフレッシュ | ctrl-r |
| `first` | 先頭アイテムへ | ナビゲーション |
| `toggle` | 選択トグル | マルチセレクト |

### キーバインドディスパッチの仕組み

全キーが `execute-silent[fzfw execute _key:{key} {q} {}]` にマップされる。
サーバーが `_key:` プレフィックスを検知 → 現在モードの `rendered_bindings[key]` を参照 →
fzf に POST で適切なアクションを返す。
これにより fzf のバインドを再構築せずにモード切替が可能。

### JSON-RPC メソッド (3種)

| メソッド | パラメータ | 用途 |
|---------|-----------|------|
| **Load** | `registered_name`, `query`, `item` | アイテムのストリーミング読み込み (100件チャンク) |
| **Preview** | `item` + preview window メタデータ | プレビュー表示 |
| **Execute** | `registered_name`, `query`, `item` | コールバック/キーバインド実行 |

### FzfClient API

```
FzfClient::post_action(&self, action: &str)
  → HTTP/1.0 POST を fzf の --listen ソケットに送信
  → `+` 区切りで複合アクション可 (例: "reload[...]+clear-query")
```

### モード一覧 (27モード)

| カテゴリ | モード |
|---------|-------|
| ファイル | `menu`, `fd`, `mru`, `visits`, `bookmark`, `mark`, `buffer` |
| Git | `git-branch`, `git-log`, `git-reflog`, `git-diff`, `git-status` |
| GitHub | `pr-list`, `pr-threads`, `pr-diff` |
| 検索 | `livegrep`, `livegrepf`, `livegrep(no-ignore)`, `diagnostics` |
| ナビゲーション | `zoxide`, `nvim-session`, `browser-history`, `browser-bookmark` |
| カスタム | `runner`, `runner-commands` |

### ModeDef トレイト

```rust
pub trait ModeDef: AsAny {
    fn name(&self) -> &'static str;
    fn fzf_prompt(&self) -> String;           // デフォルト: "{name}>"
    fn fzf_bindings(&self) -> (ModeBindings, CallbackMap);
    fn mode_enter_actions(&self) -> Vec<fzf::Action>;  // モード切替時の追加アクション
    fn wants_sort(&self) -> bool;             // デフォルト: true
    fn load<'a>(...) -> LoadStream<'a>;       // ストリーミングロード
    fn preview<'a>(...) -> BoxFuture<'a, ...>;
    fn execute<'a>(...) -> BoxFuture<'a, ...>; // オプション
}
```

### 未使用の主要 fzf 機能 (0.30〜0.71)

ワクワクセクションで活用候補となる機能:

| 機能 | バージョン | 概要 |
|------|-----------|------|
| `transform()` | 0.45 | 外部コマンドの出力に基づく条件付きアクションディスパッチ |
| `become()` | 0.38 | fzf プロセスを別コマンドに置換 |
| `trigger()` | 0.65 | 他のキー/イベントのバインドをプログラム的に発火 |
| `search()` / `transform-search()` | 0.59 | 任意のクエリで検索を発火 |
| `--no-input` / `hide-input` | 0.59 | 入力セクションの非表示 |
| Multi-line items | 0.53 | 複数行にまたがるアイテム表示 |
| `--raw` | 0.66 | 全アイテム表示 (非マッチは dim) |
| `--tmux` | 0.53 | ネイティブ tmux popup 統合 |
| `print(...)` | 0.53 | 終了時に任意文字列を出力 |
| `--accept-nth` | 0.60 | 出力フィールド選択 |
| `--footer` / `change-footer` | 0.63 | フッターセクション |
| `bg-transform-*` | 0.63 | 非同期バックグラウンド transform |
| `--style=full` | 0.58 | セクション別ボーダー/ラベル |
| `--id-nth` | 0.71 | reload 時のアイテム同一性追跡 |
| `--ghost` | 0.61 | 空クエリ時のプレースホルダ |
| `--highlight-line` | 0.52 | 現在行の全体ハイライト |
| `--wrap` / `--wrap=word` | 0.54 / 0.68 | 行折り返し |
| `--gap` | 0.56 | アイテム間のスペース |
| `--freeze-left` / `--freeze-right` | 0.67 | カラム固定表示 |
| `alt-bg` | 0.62 | ストライプ (交互背景色) |
| Image support (Sixel/Kitty/iTerm2) | 0.44 | プレビュー内画像表示 |
| `change-multi` | 0.51 | マルチセレクトの動的切替 |
| `toggle-bind` | 0.59 | キーバインドの有効/無効切替 |
| `exclude` / `exclude-multi` | 0.60 | アイテムの動的除外 |
| `change-nth` | 0.58 | 検索対象フィールドの動的変更 |
| `change-with-nth` | 0.70 | 表示フィールドの動的変更 |
| `reload-sync` | 0.36 | 同期リロード (完了まで旧リスト保持) |
| `--info-command` | 0.54 | info 行のカスタムコマンド |
| `pos(N)` | 0.36 | カーソルを N 番目に移動 |
| `GET /` state endpoint | 0.43 | fzf の現在状態を JSON で取得 |
| `--listen` Unix domain socket | 0.66 | `.sock` パスで Unix ソケット対応 |

### 主要イベント (未使用)

| イベント | バージョン | 発火タイミング |
|---------|-----------|--------------|
| `start` | 0.35 | fzf 起動時 (1回) |
| `load` | 0.36 | 入力ストリーム完了時 |
| `focus` | 0.37 | フォーカスアイテム変更時 |
| `result` | 0.46 | フィルタリング完了時 |
| `resize` | 0.46 | ターミナルサイズ変更時 |
| `zero` | 0.40 | マッチ 0 件時 |
| `one` | 0.39 | マッチ 1 件時 |
| `multi` | 0.64 | マルチセレクト変更時 |
| `click-header` | 0.52 | ヘッダクリック時 |
| `click-footer` | 0.65 | フッタークリック時 |

### 主要環境変数 (未使用)

| 変数 | バージョン | 内容 |
|-----|-----------|------|
| `$FZF_MATCH_COUNT` | 0.46 | マッチ件数 |
| `$FZF_SELECT_COUNT` | 0.46 | 選択件数 |
| `$FZF_TOTAL_COUNT` | 0.46 | 全件数 |
| `$FZF_POS` | 0.51 | カーソル位置 (1-based) |
| `$FZF_QUERY` | 0.46 | 現在のクエリ |
| `$FZF_PROMPT` | 0.46 | 現在のプロンプト |
| `$FZF_KEY` | 0.50 | 最後に押されたキー |
| `$FZF_ACTION` | 0.46 | 最後のアクション名 |
| `$FZF_LINES` / `$FZF_COLUMNS` | 0.46 | fzf のサイズ |
| `$FZF_SOCK` | 0.66 | Unix ソケットパス |
