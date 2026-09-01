use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::from_filename(".env.local");
    let _ = dotenvy::from_filename(".env");
    heddle::headless::run_headless().await
}
