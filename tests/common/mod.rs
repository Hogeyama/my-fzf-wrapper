#![deny(warnings)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

pub fn which(cmd: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|p| p.join(cmd).exists()))
        .unwrap_or(false)
}

pub struct MockFzf {
    _dir: tempfile::TempDir,
    bin_dir: PathBuf,
}

impl MockFzf {
    pub fn new() -> Self {
        let dir = tempfile::TempDir::new().unwrap();
        let bin_dir = dir.path().to_path_buf();
        let path = bin_dir.join("fzf");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(
            f,
            "#!/usr/bin/env bash
set -euo pipefail
# サーバーがすぐ終了しないよう短時間だけ待つ
sleep 3
"
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perm = std::fs::metadata(&path).unwrap().permissions();
            perm.set_mode(0o755);
            std::fs::set_permissions(&path, perm).unwrap();
        }
        Self { _dir: dir, bin_dir }
    }

    pub fn prepend_path_env(&self) -> String {
        let old_path = std::env::var("PATH").unwrap_or_default();
        format!("{}:{}", self.bin_dir.display(), old_path)
    }
}

pub struct HeadlessNvim {
    pub child: Child,
    pub sock: PathBuf,
}

impl HeadlessNvim {
    pub fn spawn(sock: PathBuf) -> Option<Self> {
        if !which("nvim") {
            eprintln!("nvim not found; skip integration test");
            return None;
        }
        let mut child = Command::new("nvim")
            .args([
                "--headless",
                "--clean",
                "--cmd",
                "set shortmess+=F",
                "--listen",
                sock.to_str().unwrap(),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;

        if wait_for_socket(&sock, Duration::from_secs(2)) {
            Some(Self { child, sock })
        } else {
            let _ = child.kill();
            eprintln!("nvim socket not created; skip integration test");
            None
        }
    }
}

pub struct ServerProc {
    pub child: Child,
}

impl ServerProc {
    pub fn spawn(
        bin: &str,
        path_env: &str,
        nvim_sock: &Path,
        server_sock: &Path,
        log_base: &Path,
    ) -> Option<Self> {
        let mut child = Command::new(bin)
            .env("PATH", path_env)
            .env("NVIM_LISTEN_ADDRESS", nvim_sock.to_str().unwrap())
            .env("FZFW_TEST_SOCKET", server_sock.to_str().unwrap())
            .env("FZFW_LOG_FILE", log_base.to_str().unwrap())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;

        if wait_for_socket(server_sock, Duration::from_secs(2)) {
            Some(Self { child })
        } else {
            eprintln!("server socket not created; skip integration test");
            let _ = child.kill();
            None
        }
    }
}

pub fn wait_for_socket(path: &Path, timeout: Duration) -> bool {
    let start = Instant::now();
    while !path.exists() {
        if start.elapsed() > timeout {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    true
}

pub fn cargo_bin() -> String {
    env!("CARGO_BIN_EXE_fzfw").to_string()
}

pub struct TestHarness {
    _tmp: tempfile::TempDir,
    pub sock_path: PathBuf,
    pub bin: String,
    mock_fzf: MockFzf,
    nvim: HeadlessNvim,
    server: ServerProc,
}

impl TestHarness {
    pub fn spawn() -> Option<Self> {
        let tmp = tempfile::TempDir::new().ok()?;
        let tmp_path = tmp.path().to_path_buf();
        let sock_path = tmp_path.join("fzfw.sock");
        let log_base = tmp_path.join("fzfw-test-log");

        let mock_fzf = MockFzf::new();
        let nvim_sock = tmp_path.join("nvim.sock");
        let nvim = HeadlessNvim::spawn(nvim_sock.clone())?;

        let bin = cargo_bin();
        let path_env = mock_fzf.prepend_path_env();
        let server = ServerProc::spawn(&bin, &path_env, &nvim.sock, &sock_path, &log_base)?;

        Some(Self {
            _tmp: tmp,
            sock_path,
            bin,
            mock_fzf,
            nvim,
            server,
        })
    }

    fn path_env(&self) -> String {
        self.mock_fzf.prepend_path_env()
    }

    pub fn run_client(&self, args: &[&str]) -> Output {
        Command::new(&self.bin)
            .env("PATH", self.path_env())
            .env("NVIM", self.nvim.sock.to_str().unwrap())
            .env("FZF_PREVIEW_LINES", "10")
            .env("FZF_PREVIEW_COLUMNS", "10")
            .args(args)
            .output()
            .expect("failed to run client command")
    }

    pub fn load(&self, menu: &str, query: Option<&str>, cwd: Option<&str>) -> Output {
        self.run_client(&[
            "load",
            "--fzfw-socket",
            self.sock_path.to_str().unwrap(),
            menu,
            query.unwrap_or(""),
            cwd.unwrap_or(""),
        ])
    }

    pub fn preview(&self, item: &str) -> Output {
        self.run_client(&[
            "preview",
            "--fzfw-socket",
            self.sock_path.to_str().unwrap(),
            item,
        ])
    }

    pub fn execute(&self, registered_name: &str, item: &str, query: Option<&str>) -> Output {
        self.run_client(&[
            "execute",
            "--fzfw-socket",
            self.sock_path.to_str().unwrap(),
            registered_name,
            item,
            query.unwrap_or(""),
        ])
    }

    pub fn change_mode(&self, mode: &str, query: Option<&str>) -> Output {
        self.run_client(&[
            "change-mode",
            "--fzfw-socket",
            self.sock_path.to_str().unwrap(),
            mode,
            query.unwrap_or(""),
        ])
    }

    pub fn change_directory(&self, path: &str) -> Output {
        self.run_client(&[
            "change-directory",
            "--fzfw-socket",
            self.sock_path.to_str().unwrap(),
            "--dir",
            path,
        ])
    }
}

impl Drop for TestHarness {
    fn drop(&mut self) {
        let _ = self.server.child.kill();
        let _ = self.nvim.child.kill();
    }
}
