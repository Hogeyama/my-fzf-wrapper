use std::error::Error;
extern crate fzfw;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    fzfw::tokio_main().await
}
