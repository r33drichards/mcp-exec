use anyhow::Result;

mod types;
mod log_store;
mod executor;
mod service;

#[tokio::main]
async fn main() -> Result<()> {
    println!("mcp-exec server");
    Ok(())
}
