# アーキテクチャ図

## コンポーネント構成

```mermaid
graph TB
    subgraph "fzf プロセス"
        FZF[fzf]
        LISTEN["--listen (Unix socket)<br/>HTTP POST 受付"]
    end

    subgraph "fzfw Server"
        LOOP[server loop<br/>tokio::select!]
        SS[ServerState]
        FC[fzf_client: FzfClient]

        SS --- FC
        LOOP --> FC
    end

    subgraph "fzfw CLI (子プロセス)"
        CLIENT[client::send_request]
    end

    subgraph "ModeDef callbacks"
        LOAD[load]
        PREVIEW[preview]
        EXEC[execute / execute_silent]
        BIND[fzf_bindings]
    end

    FZF -->|"execute-silent/execute/reload<br/>(fzfw コマンド実行)"| CLIENT
    CLIENT -->|"JSON-RPC<br/>(Unix socket)"| LOOP
    LOOP -->|"callback 呼び出し"| EXEC
    LOOP -->|"callback 呼び出し"| LOAD
    LOOP -->|"callback 呼び出し"| PREVIEW
    FC -->|"HTTP POST<br/>(--listen socket)"| LISTEN

    style FC fill:#f96,stroke:#333
    style LISTEN fill:#f96,stroke:#333
```

## キーバインド処理フロー (execute-silent 方式)

全キーは統合バインディングで `execute-silent` にマッピングされ、
サーバーが現モードの `rendered_bindings` を参照して fzf に POST する。

```mermaid
sequenceDiagram
    participant User
    participant fzf
    participant CLI as fzfw CLI (子プロセス)
    participant Server as fzfw Server

    User->>fzf: キー押下 (例: ctrl-k)
    Note over fzf: 統合バインディング:<br/>execute-silent[fzfw execute _key:ctrl-k {q} {}]
    fzf->>CLI: fzfw execute _key:ctrl-k <query> <item>
    CLI->>Server: Execute RPC (Unix socket)
    Server->>Server: rendered_bindings["ctrl-k"] を取得
    Server->>fzf: fzf_client.post_action(action)<br/>(--listen POST)
    Server-->>CLI: Execute 応答
    Note over fzf: POST されたアクションを実行<br/>(execute-silent 完了後にキューから処理)
```

### アクセス範囲

| コンポーネント | アクセスできるもの |
|---|---|
| Server loop | `Env`, `ServerState` (fzf_client, all_modes, current_mode_name, sort_enabled 含む) |
| ModeDef callbacks | `&Env` (config, nvim, fzf_client), `&mut State` (last_load_resp) |

### サーバーハンドラ

サーバーは 3 つの RPC のみ受け付ける:

| ハンドラ | 用途 |
|---|---|
| **Load** | fzf の候補一覧をストリーム返却 |
| **Preview** | 選択中アイテムのプレビュー |
| **Execute** | `_key:` プレフィックス → rendered_bindings を POST / それ以外 → コールバック実行 |
