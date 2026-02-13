mod common;

#[test]
fn menu_load_success() {
    // ハーネス起動
    let Some(h) = common::TestHarness::spawn() else {
        panic!("failed to spawn test harness");
    };

    let output = h.load("default", Option::None, Option::None);

    assert!(output.status.success(), "client load exited with failure");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout.lines();
    let header = lines.next().unwrap_or("");
    assert!(
        header.starts_with("["),
        "header should be bracketed pwd: {}",
        header
    );
    assert!(
        header.ends_with("]"),
        "header should be bracketed pwd: {}",
        header
    );

    // menu モードの items に少なくとも fd などの既知モードが含まれる想定
    let items: Vec<&str> = lines.collect();
    assert!(
        items.contains(&"fd"),
        "items should contain 'fd' (got: {:?})",
        items
    );
    assert!(
        !items.contains(&"menu"),
        "items should not contain 'menu' (got: {:?})",
        items
    );
}
