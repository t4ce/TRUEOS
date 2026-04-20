/*!
 * Example 1: Basic API Call
 *
 * Demonstrates the simplest API call
 *
 * Run: cargo run --example basic
 */

use anyhow::Result;
use colored::*;
use serde_json::json;

#[path = "support/common.rs"]
mod common;

#[tokio::main]
async fn main() -> Result<()> {
    println!("{}", "=== Example 1: Basic API Call ===\n".cyan().bold());

    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .expect("Please set the ANTHROPIC_API_KEY environment variable");

    // Build request
    let body = json!({
        "model": "claude-opus-4-20250514",
        "max_tokens": 1024,
        "messages": [
            {
                "role": "user",
                "content": "Introduce yourself in one sentence"
            }
        ]
    });

    // Send request
    let result = common::anthropic_post(&body, &api_key).await?;

    // Extract and print response text
    if let Some(content) = result["content"][0]["text"].as_str() {
        println!("{} {}", "Claude:".green().bold(), content);
    }

    // Print usage stats
    println!("\n{}", "Usage:".cyan().bold());
    println!("  Input tokens: {}", result["usage"]["input_tokens"]);
    println!("  Output tokens: {}", result["usage"]["output_tokens"]);

    Ok(())
}
