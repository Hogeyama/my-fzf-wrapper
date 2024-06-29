mod client;
mod config;
mod logger;
mod method;
mod mode;
mod nvim;
mod server;
mod state;
mod utils;

// std
use std::env;
use std::error::Error;
use std::fs;
use std::path::Path;

// log
#[macro_use(o)]
extern crate slog;
#[macro_use]
extern crate slog_scope;

#[macro_use(json)]
extern crate serde_json;

use once_cell::sync::Lazy;
// rand
use rand::distributions::Alphanumeric;
use rand::Rng;

// clap command line parser
use clap::Parser;

// tokio
use tokio::net::UnixListener;

use crate::client::run_command;
use crate::client::Command;
use crate::config::Config;
use crate::nvim::start_nvim;

////////////////////////////////////////////////////////////////////////////////////////////////////
// Cli
////////////////////////////////////////////////////////////////////////////////////////////////////

static VERSION: Lazy<String> = Lazy::new(|| {
    let version = env!("CARGO_PKG_VERSION");
    let git_revision = option_env!("GIT_REVISION").unwrap_or("unknown");
    format!("v{} ({})", version, git_revision)
});

#[derive(Parser)]
#[clap(author, version = VERSION.as_str(), about, long_about = None)]
#[clap(propagate_version = true)]
struct Cli {
    /// (internal)
    #[clap(subcommand)]
    command: Option<Command>,

    #[clap(long, env)]
    fzfw_self: Option<String>,

    #[clap(long, env, default_value = "/tmp/fzfw")]
    fzfw_log_file: String,

    /// Address or filepath to a socket used to communicate with neovim.
    #[clap(long, env, required_unless("nvim-listen-address"))]
    nvim: Option<String>,

    /// Address or filepath to a socket used to communicate with neovim (legacy).
    #[clap(long, env)]
    nvim_listen_address: Option<String>,
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Init
////////////////////////////////////////////////////////////////////////////////////////////////////

async fn init(args: Cli) -> Result<(), Box<dyn Error>> {
    fn get_program_path() -> String {
        env::current_exe()
            .expect("$0")
            .to_string_lossy()
            .into_owned()
    }

    fn gen_socket_name() -> String {
        format!(
            "/tmp/{}.sock",
            rand::thread_rng()
                .sample_iter(&Alphanumeric)
                .take(10)
                .map(char::from)
                .collect::<String>()
        )
    }

    fn create_listener(socket_name: &str) -> UnixListener {
        let sockfile = Path::new(socket_name);
        if sockfile.exists() {
            fs::remove_file(sockfile).expect("Failed to remove old socket");
        }

        UnixListener::bind(sockfile).expect("Failed to bind socket")
    }

    let nvim = start_nvim(&args.nvim.or(args.nvim_listen_address).unwrap())
        .await
        .map_err(|e| e.to_string())?;

    let socket_name = gen_socket_name();
    let socket = create_listener(&socket_name);

    let myself = args.fzfw_self.unwrap_or(get_program_path());
    let config = config::new(
        myself.clone(),
        nvim,
        socket_name.clone(),
        args.fzfw_log_file,
    );
    let state = state::State::new();

    server::server(config, state, socket)
        .await
        .unwrap_or_else(|e| {
            error!("server: error"; "error" => e);
        });

    // 後始末
    fs::remove_file(&socket_name).expect("Failed to remove socket");

    Ok(())
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Main
////////////////////////////////////////////////////////////////////////////////////////////////////

pub async fn tokio_main() -> Result<(), Box<dyn Error>> {
    let args = Cli::parse();
    match args.command {
        None => {
            let _guard = logger::init(&format!("{}-server.log", args.fzfw_log_file))?;
            init(args).await
        }
        Some(command) => {
            let _guard = logger::init(&format!("{}-client.log", args.fzfw_log_file))?;
            run_command(command).await
        }
    }
}
