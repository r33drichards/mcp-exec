use anyhow::Result;

mod types;
mod log_store;
mod executor;

#[tokio::main]
async fn main() -> Result<()> {
    println!("mcp-exec server");
    Ok(())
}
