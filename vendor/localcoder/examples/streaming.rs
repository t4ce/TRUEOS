/*!
 * Example 2: Streaming Response
 *
 * Demonstrates incremental display using the shared transport layer.
 * TRUEOS/native embedding currently exposes buffered fetches, so this example
 * prints the final text progressively instead of consuming raw SSE chunks.
 *
 * Run: cargo run --example streaming
 */

use anyhow::Result;
use colored::*;
use serde_json::json;
use std::{thread, time::Duration};

#[path = "support/common.rs"]
mod common;

#[tokio::main]
async fn main() -> Result<()> {
    println!(
        "{}",
        "=== Example 2: Streaming Response ===\n".cyan().bold()
    );

    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .expect("Please set the ANTHROPIC_API_KEY environment variable");

    let body = json!({
        "model": "claude-opus-4-20250514",
        "max_tokens": 1024,
        "messages": [
            {
                "role": "user",
                "content": "Write a short poem about Rust programming (4 lines)"
            }
        ],
        "stream": false
    });

    let result = common::anthropic_post(&body, &api_key).await?;
    let content = result["content"][0]["text"].as_str().unwrap_or_default();

    println!("{}", "Claude: ".green().bold());

    for ch in content.chars() {
        print!("{}", ch);
        let _ = std::io::Write::flush(&mut std::io::stdout());
        thread::sleep(Duration::from_millis(12));
    }

    println!("\n");
    Ok(())
}
