use std::error::Error;

// Neovim
use nvim_rs::compat::tokio::Compat as TokioCompat;
use nvim_rs::create::tokio as nvim_tokio;
use nvim_rs::Handler;

// Tokio
use parity_tokio_ipc::Connection;
use tokio::io::WriteHalf;

#[derive(Clone)]
struct NeovimHandler {}

pub fn _to_nvim_error(err: impl ToString) -> rmpv::Value {
    rmpv::Value::String(rmpv::Utf8String::from(err.to_string()))
}

impl Handler for NeovimHandler {
    type Writer = TokioCompat<WriteHalf<Connection>>;
}

pub async fn start_nvim(nvim_listen_address: &str) -> Result<Neovim, Box<dyn Error>> {
    let handler: NeovimHandler = NeovimHandler {};
    let (nvim, _io_handler) = nvim_tokio::new_path(nvim_listen_address, handler)
        .await
        .expect("Connect to nvim failed");
    Ok(nvim)
}

pub type Neovim = nvim_rs::Neovim<TokioCompat<WriteHalf<Connection>>>;
