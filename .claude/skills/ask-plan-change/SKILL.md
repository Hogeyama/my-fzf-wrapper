---
name: ask-plan-change
description: When a plan created in plan mode cannot be executed as originally planned, detect the deviation, explain the reason clearly, and ask the user for guidance on how to proceed. Use this skill when implementation discovers issues that require changing the original plan (unexpected technical constraints, missing context, architectural problems, etc.). Displays questions in a tmux popup for immediate visibility. The skill handles decision-making about when to deviate and what to ask.
---

# Ask Plan Change

## Overview

During plan mode work, implementation may reveal issues that require deviating from the original
plan. This skill enables Claude to:

1. **Detect deviations** - Recognize when the planned approach won't work as designed
2. **Explain clearly** - Articulate why the original plan needs to change
3. **Ask for guidance** - Present options to the user via tmux popup
4. **Respect user decisions** - Let the user choose the path forward, not Claude

The skill uses a tmux popup to ensure the user sees the question immediately, even if they're not
actively watching the terminal.

## When to Use This Skill

Trigger this skill when:

- **Technical constraints discovered** - The planned approach hits a limitation (missing API,
  incompatible library version, system requirement)
- **Missing context** - Information needed for the plan wasn't available in the original planning
  phase
- **Architectural conflict** - The plan conflicts with existing project structure or patterns (see
  `docs/architecture.md` or `docs/profile-cli-spec.md`)
- **Significant scope change** - The work requires different effort/approach than originally planned
- **Blocking dependencies** - The plan depends on work that can't be completed in the current
  context

**Do NOT trigger** for minor adjustments or implementation details that don't affect the core plan.

## How to Use This Skill

### Step 1: Recognize the Deviation

When you discover the plan won't work, clearly identify:

- What the plan expected
- What the actual situation is
- Why they don't align

### Step 2: Explain and Ask

Present the issue to the user with:

1. **Brief summary** of why the plan needs to change
2. **Context** - What you discovered and why it matters
3. **Options** - 2-4 possible paths forward with trade-offs
4. **Your recommendation** (optional) - If there's a clear best path

### Step 3: Use the Script

Call `scripts/ask_user_popup.sh` with a heredoc to display the question in a tmux popup:

```bash
scripts/ask_user_popup.sh <<'EOF'
[Question text - multiple lines OK]
---
Option 1: [description]
Option 2: [description]
Option 3: [description]
EOF
```

Format: Lines before `---` are the question. Lines after are options (one per line).

The user can:

- Enter a number to select an option
- Enter `e` to open `$EDITOR` for a free-form response (useful for detailed feedback)

### Step 4: Follow User's Decision

The user's response guides what happens next:

- Modify the plan accordingly
- Continue implementation with the chosen approach
- Document the change in context

## Example Scenario

**Original plan:** "Refactor auth system to use JWT tokens stored in HttpOnly cookies"

**Discovery during implementation:** "The current architecture uses context-based session
management, and switching to JWT would require changes to 8 different components across the
codebase"

**Script invocation:**

```bash
scripts/ask_user_popup.sh <<'EOF'
計画からの逸脱: JWT token アプローチは大規模リファクタリングが必要

現状のアーキテクチャは context-based session management を使用しており、
JWT に切り替えるには 8 つのコンポーネントの変更が必要です。
---
JWT で進める（スコープ拡大、長期的にはベター）
現行セッションを維持し HttpOnly cookie 対応を追加
シンプルな cookie で進める（HttpOnly は後回し）
EOF
```

## Resources

### scripts/ask_user_popup.sh

Shell script that displays a question in a tmux popup and captures the user's response.

**Usage:**

```bash
scripts/ask_user_popup.sh <<'EOF'
Question text (multi-line OK)
---
Option 1
Option 2
Option 3
EOF
```

**Returns:** The selected option text, or the user's free-form response (if editor was used)

**Features:**

- Heredoc input for easy multi-line questions
- `e` option opens `$EDITOR` for free-form responses
- Auto-detects user's tmux session (avoids Claude's session)
- Auto-sizes popup based on content
