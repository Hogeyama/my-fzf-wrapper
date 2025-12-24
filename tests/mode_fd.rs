mod common;

use std::fs;
// use std::io::Write;

#[test]
fn load_success() {
    let Some(h) = common::TestHarness::spawn() else {
        assert!(false, "failed to spawn test harness");
        return;
    };

    // Create some files in the temp directory to be found by fd
    // We need to use the harness temp dir, but TestHarness doesn't expose it directly except via sock_path's parent.
    // Let's create a file in the same dir as where we successfully "change_directory" or just where fd runs.

    // TestHarness uses a temp dir. "fd" lists current directory.
    // The server/client share the machine but run in separate processes.
    // We should change directory to a controlled place.
    // Is there a way to get harness root?
    // h.sock_path is in the temp dir.
    let root = h.sock_path.parent().unwrap();

    let file1 = root.join("file1.txt");
    let file2 = root.join("file2.txt");
    fs::write(&file1, "content1").unwrap();
    fs::write(&file2, "content2").unwrap();

    // change directory to root
    let output = h.change_directory(root.to_str().unwrap());
    assert!(output.status.success());

    // Switch to fd mode
    let output = h.change_mode("fd", None);
    assert!(output.status.success());

    // Load
    let output = h.load("default", None, None);
    assert!(output.status.success(), "client load exited with failure");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines = stdout.lines();

    // The first line might be a header or not depending on implementation of LoadResp::wip_with_default_header
    // In server.rs, it sends header.
    // fd mode sends items.

    let items: Vec<&str> = lines.collect();

    // Check if created files are in the list
    assert!(
        items.iter().any(|&x| x.contains("file1.txt")),
        "items should contain file1.txt: {:?}",
        items
    );
    assert!(
        items.iter().any(|&x| x.contains("file2.txt")),
        "items should contain file2.txt: {:?}",
        items
    );
}

#[test]
fn preview_success() {
    let Some(h) = common::TestHarness::spawn() else {
        assert!(false, "failed to spawn test harness");
        return;
    };

    let root = h.sock_path.parent().unwrap();
    let file1 = root.join("file1.txt");
    fs::write(&file1, "PREVIEW_CONTENT").unwrap();

    // change directory to root
    let output = h.change_directory(root.to_str().unwrap());
    assert!(output.status.success());

    // Switch to fd mode
    let output = h.change_mode("fd", None);
    assert!(output.status.success());

    // Preview
    // Note: fd mode preview uses "bat" or cat.
    // We need to make sure we are referencing the file correctly.
    // If we are in 'root', passing "file1.txt" should work.
    let output = h.preview("file1.txt");
    assert!(
        output.status.success(),
        "client preview exited with failure"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    // output should contain the content
    assert!(
        stdout.contains("PREVIEW_CONTENT"),
        "preview should contain file content. Got: {}",
        stdout
    );
}
