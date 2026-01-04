use anyhow::Result;

mod types;
mod log_store;

#[tokio::main]
async fn main() -> Result<()> {
    println!("mcp-exec server");
    Ok(())
}
