use std::error::Error;
extern crate myfzf_wrapper_rs;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    myfzf_wrapper_rs::tokio_main().await
}
