---
name: implement-with-review
description: 実装後に自動でレビューと修正を繰り返し、品質を担保するワークフロー
user-invocable: true
argument-hint: "[実装内容の説明]"
---

# Enterprise Implement-with-Review

計画→レビュー→実装(1コミット単位)のループで品質を担保するワークフロー。

## 原則

- **orchestrator（あなた）はコードの読み書きを一切しない。** 初期調査も含めすべてサブエージェントに委任する。
- **orchestrator のコンテキストは貴重。** サブエージェントからは判定（approve/reject/pass/fail/done/blocked）のみ受け取る。詳細はすべてファイル経由。
- **1 implementer = 1 コミット。** プラン外の作業は禁止。

## ファイルベースの受け渡し

サブエージェント間のデータは `.claude/workflow/` ディレクトリを介してやり取りする。
orchestrator はこのディレクトリ内のファイルを **読まない**（ユーザーに計画を提示するときのみ例外）。

```
.claude/workflow/
  plan.md                  # planner の出力（計画、再計画時は上書き）
  plan-review-1.md         # plan-reviewer の findings（連番）
  e2e-plan-review-1.md     # plan review 時の build-tester 結果（連番）
  e2e-final.md             # 最終 build-tester 結果
  code-review-1.md         # code-reviewer の findings（Commit番号で連番）
  code-review-2.md
  code-review-3-fix1.md    # 修正後の再レビュー（-fixN で区別）
  ...
```

ファイル名の連番ルール:
- **plan-review**: `plan-review-{iteration}.md` (plan loop の iteration)
- **e2e-plan-review**: `e2e-plan-review-{iteration}.md`
- **code-review**: `code-review-{commit番号}.md`、修正後の再レビューは `code-review-{commit番号}-fix{N}.md`

各サブエージェントは:
- **入力**: 指定されたファイルパスから前のエージェントの出力を読む
- **出力**: 指定されたファイルパスに詳細を書き、orchestrator には **判定のみ** 返す

## サブエージェント

エージェント定義ファイルは `.claude/skills/implement-with-review/agents/` にある。
各エージェントは Agent ツールの `subagent_type` で呼び出す。**全エージェントを省略せず呼ぶこと。**

| 役割 | subagent_type | 定義ファイル | isolation | 説明 |
|------|--------------|-------------|-----------|------|
| planner | `planner` | `agents/planner.md` | - | コードベース調査 + コミット粒度の計画作成 |
| plan-reviewer | `plan-reviewer` | `agents/plan-reviewer.md` | `worktree` | worktree でスパイク実装して計画を検証 (approve/reject) |
| build-tester | `general-purpose` | `agents/build-tester.md` | - | E2E テストでユーザー目線の動作を検証 (pass/fail) |
| implementer | `implementer` | `agents/implementer.md` | - | 1コミット分の実装 |
| code-reviewer | `code-reviewer` | `agents/code-reviewer.md` | - | コミットのコードレビュー |

各サブエージェントの詳細な手順・出力フォーマット・制約は定義ファイルに記載済み。
orchestrator は定義ファイルの内容を把握する必要はなく、タスク内容とファイルパスを渡すだけでよい。

## ワークフロー

```
User request
  ↓
[Step 1] 設定読み込み (review-config.yml) + workflow ディレクトリ初期化
  ↓
[Step 2] Plan Loop
  planner → plan.md に出力
  plan-reviewer → plan.md を読み、スパイク実装、plan-review.md に出力
                → orchestrator には approve/reject のみ返す
  build-tester → スパイクの worktree で e2e 実行、e2e-plan-review.md に出力
             → orchestrator には pass/fail のみ返す
  → approve かつ pass? → plan.md を読んでユーザーに提示 → Step 3
  → reject or fail?   → planner に再計画を依頼（planner が plan-review.md / e2e-plan-review.md を読む）
  → N回失敗?          → ユーザーに判断を委ねる
  ↓
[Step 3] Commit Loop (計画の各コミットについて)
  implementer → plan.md を読み、1コミット分実装
             → orchestrator には done/blocked のみ返す
  → done?    → code-reviewer → code-review.md に出力、orchestrator には approve/reject + 件数のみ返す
             → approve? → /commit → 次のコミット
             → reject?  → implementer に修正依頼（implementer が code-review.md を読む）
  → blocked? → planner に再計画依頼 → Step 2 に戻る
  ↓
[Step 4] E2E 検証
  build-tester → e2e-final.md に出力、orchestrator には pass/fail のみ返す
  → pass? → Step 5
  → fail? → planner に修正計画依頼（planner が e2e-final.md を読む） → Step 3 の続き
  ↓
[Step 5] 完了報告
```

### Step 1: 設定の読み込み

1. `./review-config.yml` を読み、以下を確認:
   - `max_plan_iterations`: 計画フェーズのループ上限
   - `max_review_iterations`: 各コミットのレビューループ上限
   - `stop_when`: コードレビューのデフォルト終了条件
   - `rules`: レビュー観点（ルールごとに `stop_when` を上書き可能）

2. `.claude/workflow/` ディレクトリを作成（`mkdir -p`）

### Step 2: Plan Loop

1. **planner** を呼ぶ。渡す情報:
   - ユーザーの実装指示
   - 出力先: `.claude/workflow/plan.md`
   - (再計画の場合) 「`.claude/workflow/plan-review-{前回iteration}.md` と `.claude/workflow/e2e-plan-review-{前回iteration}.md` を読んで前回の指摘を踏まえよ」
   - (差し戻しの場合) blocked reason の概要 + 完了済みコミット情報

2. **plan-reviewer** を `isolation: "worktree"` で呼ぶ。渡す情報:
   - ユーザーの元の指示
   - 計画ファイル: `.claude/workflow/plan.md`（自分で読め）
   - 出力先: `.claude/workflow/plan-review-{iteration}.md`（iteration は plan loop の回数）

3. **build-tester** を plan-reviewer の worktree で呼ぶ。渡す情報:
   - ユーザーの元の指示
   - 計画ファイル: `.claude/workflow/plan.md`（自分で読め）
   - worktree パス
   - 出力先: `.claude/workflow/e2e-plan-review-{iteration}.md`
   - **テストケースの指示は出さない。** 何をテストするかは build-tester が自分で判断する

4. 判定（orchestrator はサブエージェントの返り値 approve/reject, pass/fail のみで判断）:
   - **approve かつ pass** → 5 へ
   - **いずれか reject/fail** → planner を再度呼ぶ (1 に戻る)
   - **max_plan_iterations 到達** → `.claude/workflow/plan.md` を読んでユーザーに提示し判断を委ねる

5. **ユーザーに計画を提示して確認を取る。** `.claude/workflow/plan.md` を読んでコミット一覧を見せ、以下を問う:
   - **LGTM** → Step 3 へ
   - **修正指示あり** → ユーザーのフィードバックを planner に渡して再計画 (1 に戻る)

### Step 3: Commit Loop

承認された計画の各コミットについて:

1. **implementer** を呼ぶ。渡す情報:
   - 計画ファイル: `.claude/workflow/plan.md`（自分で読め）
   - 今回実装する Commit 番号
   - (修正の場合)「`.claude/workflow/code-review-{commit番号}[-fix{N-1}].md` を読んで指摘を修正せよ」

2. 結果を処理（返り値の done/blocked のみで判断）:
   - **done** → 3 へ
   - **blocked** → reason の概要を添えて planner に残り計画を練り直させる (Step 2 に戻る、完了済みコミットは維持)

3. **code-reviewer** を呼ぶ。渡す情報:
   - review-config.yml のパス（自分で読め）
   - 計画ファイル: `.claude/workflow/plan.md`
   - 今回の Commit 番号
   - 出力先: `.claude/workflow/code-review-{commit番号}.md`（修正後の再レビューは `code-review-{commit番号}-fix{N}.md`）

4. レビュー結果の判定（返り値の approve/reject + severity 別件数で判断）:
   各ルールの `stop_when` (なければグローバルの `stop_when`) を適用:
   - `no_critical_findings`: そのルールの critical findings がなければ OK
   - `no_findings`: そのルールの findings が一切なければ OK
   判定:
   - **全ルール OK** → `/commit` スキルでコミット → 次の Commit へ
   - **いずれかのルールが NG** → implementer に修正依頼 (1 に戻る、max_review_iterations まで)
   - 修正ループ上限到達 → ユーザーに判断を委ねる

### Step 4: E2E 検証

全コミット完了後:

1. **build-tester** を呼ぶ。渡す情報:
   - ユーザーの元の指示
   - 計画ファイル: `.claude/workflow/plan.md`（自分で読め）
   - 実装された全コミットの概要（git log から）
   - 出力先: `.claude/workflow/e2e-final.md`
   - **テストケースの指示は出さない。** 何をテストするかは build-tester が自分で判断する

2. 判定（返り値の pass/fail のみで判断）:
   - **pass** → Step 5 へ
   - **fail** → planner に修正計画依頼（planner が `.claude/workflow/e2e-final.md` を読む）。追加コミットとして Step 3 を継続

### Step 5: 完了報告

全コミット完了後:
- 実行されたコミット一覧 (hash + message) — `git log` から取得
- 各コミットのレビュー結果サマリー（件数のみ）
- e2e テスト結果 (pass/fail)
- 残った warnings/info があれば件数を報告
