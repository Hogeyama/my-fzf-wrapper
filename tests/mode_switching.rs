mod common;

/// menu モードの load 後、mode switch して fd モードの load が成功する
#[test]
fn switch_to_fd_and_load() {
    let Some(h) = common::TestHarness::spawn() else {
        eprintln!("failed to spawn test harness; skipping");
        return;
    };

    // 初期状態 (menu) の load
    let output = h.load("default", None, None);
    assert!(output.status.success(), "initial load failed");

    // menu の enter で fd モードへ切り替え (execute_silent)
    // fd モードの load を直接呼ぶ
    let output = h.load("default", Some(""), None);
    assert!(
        output.status.success(),
        "fd mode load failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// preview で menu モードの item をプレビューすると "No description" が返る
#[test]
fn menu_preview_returns_no_description() {
    let Some(h) = common::TestHarness::spawn() else {
        eprintln!("failed to spawn test harness; skipping");
        return;
    };

    let output = h.preview("fd");
    assert!(output.status.success(), "preview failed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("No description"),
        "expected 'No description' in preview output, got: {}",
        stdout
    );
}

/// 複数回 load を呼んでもサーバーがクラッシュしない
#[test]
fn multiple_loads_do_not_crash_server() {
    let Some(h) = common::TestHarness::spawn() else {
        eprintln!("failed to spawn test harness; skipping");
        return;
    };

    for i in 0..5 {
        let output = h.load("default", None, None);
        assert!(
            output.status.success(),
            "load iteration {} failed: {}",
            i,
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

/// load の stdout にはヘッダ行 + アイテム行が含まれる
#[test]
fn load_output_has_header_and_items() {
    let Some(h) = common::TestHarness::spawn() else {
        eprintln!("failed to spawn test harness; skipping");
        return;
    };

    let output = h.load("default", None, None);
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();

    // 最低でもヘッダ1行 + items 1行
    assert!(
        lines.len() >= 2,
        "expected at least 2 lines, got {} lines: {:?}",
        lines.len(),
        lines
    );

    // ヘッダは [pwd] 形式
    assert!(lines[0].starts_with('['), "header: {}", lines[0]);
    assert!(lines[0].ends_with(']'), "header: {}", lines[0]);
}

/// menu モードの load には主要モードが含まれる
#[test]
fn menu_load_contains_expected_modes() {
    let Some(h) = common::TestHarness::spawn() else {
        eprintln!("failed to spawn test harness; skipping");
        return;
    };

    let output = h.load("default", None, None);
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let items: Vec<&str> = stdout.lines().skip(1).collect(); // skip header

    let expected_modes = [
        "fd",
        "git-branch",
        "git-status",
        "git-log",
        "livegrep",
        "zoxide",
    ];

    for mode in &expected_modes {
        assert!(
            items.contains(mode),
            "expected mode '{}' in menu items, got: {:?}",
            mode,
            items
        );
    }
}

/// バックスタックが空の状態で _key:ctrl-b を execute してもパニックしない
#[test]
fn execute_with_unknown_key_does_not_crash() {
    let Some(h) = common::TestHarness::spawn() else {
        eprintln!("failed to spawn test harness; skipping");
        return;
    };

    // 初期 load
    let output = h.load("default", None, None);
    assert!(output.status.success(), "initial load failed");

    // バックスタックが空の状態で ctrl-b (バックスタック pop) を呼ぶ
    let output = h.run_client(&[
        "execute",
        "--fzfw-socket",
        h.sock_path.to_str().unwrap(),
        "_key:ctrl-b",
        "", // query
        "", // item
    ]);
    // パニックせず正常終了すること
    assert!(
        output.status.success(),
        "execute _key:ctrl-b should not crash: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// cursor_pos 付きで execute を呼んでも正常動作する
#[test]
fn execute_param_cursor_pos_accepted() {
    let Some(h) = common::TestHarness::spawn() else {
        eprintln!("failed to spawn test harness; skipping");
        return;
    };

    // 初期 load
    let output = h.load("default", None, None);
    assert!(output.status.success(), "initial load failed");

    // cursor_pos 付きで _key:ctrl-b を呼ぶ
    let output = h.run_client(&[
        "execute",
        "--fzfw-socket",
        h.sock_path.to_str().unwrap(),
        "_key:ctrl-b",
        "",   // query
        "",   // item
        "42", // cursor_pos
    ]);
    assert!(
        output.status.success(),
        "execute with cursor_pos should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// preview を複数回呼んでもサーバーがクラッシュしない
#[test]
fn multiple_previews_stable() {
    let Some(h) = common::TestHarness::spawn() else {
        eprintln!("failed to spawn test harness; skipping");
        return;
    };

    for item in &["fd", "git-branch", "livegrep", "nonexistent_item"] {
        let output = h.preview(item);
        assert!(
            output.status.success(),
            "preview '{}' failed: {}",
            item,
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
