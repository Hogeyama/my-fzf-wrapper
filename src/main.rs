mod client;
mod config;
mod external_command;
mod logger;
mod method;
mod mode;
mod nvim;
mod server;
mod types;

// std
use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::fs;
use std::path::Path;

// log
#[macro_use(o)]
extern crate slog;
#[macro_use]
extern crate slog_scope;

// rand
use rand::distributions::Alphanumeric;
use rand::Rng;

// clap command line parser
use clap::Parser;

// serde
use serde_json::json;

// tokio
use tokio::net::UnixListener;

use crate::client::run_command;
use crate::client::Command;
use crate::config::Config;
use crate::logger::Serde;
use crate::method::LoadParam;
use crate::nvim::start_nvim;
use crate::types::Mode;

////////////////////////////////////////////////////////////////////////////////////////////////////
// Cli
////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
#[clap(propagate_version = true)]
struct Cli {
    /// (internal)
    #[clap(subcommand)]
    command: Option<Command>,

    #[clap(long, env)]
    myfzf_self: Option<String>,

    /// Address or filepath to a socket used to communicate with neovim.
    #[clap(long, env)]
    nvim_listen_address: String,
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
        if true {
            // ใในใ็จ
            "/tmp/test.sock".to_string()
        } else {
            format!(
                "/tmp/{}.sock",
                rand::thread_rng()
                    .sample_iter(&Alphanumeric)
                    .take(10)
                    .map(char::from)
                    .collect::<String>()
            )
        }
    }
    fn create_listener(socket_name: &str) -> UnixListener {
        let sockfile = Path::new(socket_name);
        if sockfile.exists() {
            fs::remove_file(&sockfile).expect("Failed to remove old socket");
        }
        let listener = UnixListener::bind(sockfile).expect("Failed to bind socket");
        listener
    }

    let config = {
        let fd: Box<dyn Mode + Send + Sync> = Box::new(mode::fd::new());
        Config {
            modes: HashMap::from([("fd".to_string(), fd)]),
        }
    };

    let nvim = start_nvim(&args.nvim_listen_address)
        .await
        .map_err(|e| e.to_string())?;

    let socket_name = gen_socket_name();
    let socket = create_listener(&socket_name);

    // start server
    let server_handler = tokio::spawn(async move {
        let initial_state = types::State {
            pwd: env::current_dir().unwrap(),
            mode: config.get_mode("fd"),
            last_load: LoadParam {
                mode: "fd".to_string(),
                args: vec![],
            },
            nvim,
        };
        let r = server::server(&config, initial_state, socket).await;
        if let Err(e) = r {
            error!("server: error"; "error" => e);
        }
    });

    // spawn fzf
    let myself = args.myfzf_self.unwrap_or(get_program_path());
    external_command::fzf::new(myself, &socket_name)
        .spawn()
        .expect("Failed to spawn fzf")
        .wait()
        .await?;

    // stop the server
    server_handler.abort();
    match server_handler.await {
        Ok(()) => {}
        Err(joinerr) if joinerr.is_cancelled() => {}
        Err(joinerr) => eprintln!("Error joining IO loop: '{}'", joinerr),
    }

    // ๅพๅงๆซ
    fs::remove_file(&socket_name).expect("Failed to remove socket");

    Ok(())
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Main
////////////////////////////////////////////////////////////////////////////////////////////////////

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let _guard = logger::init()?;
    let args = Cli::parse();
    match args.command {
        None => init(args).await,
        Some(command) => run_command(command).await,
    }
}
