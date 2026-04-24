/*!
 * Example 4: Custom Model Parameters
 *
 * Demonstrates how to customize model, temperature, and other parameters
 *
 * Run: cargo run --example custom_model
 */

use anyhow::Result;
use colored::*;
use serde_json::json;

#[path = "support/common.rs"]
mod common;

#[tokio::main]
async fn main() -> Result<()> {
    println!("{}", "=== Example 4: Custom Model Parameters ===\n".cyan().bold());

    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .expect("Please set the ANTHROPIC_API_KEY environment variable");

    // Different parameter configurations
    let configs = vec![
        ("claude-sonnet-4-20250514", 0.0, "Most conservative (temperature=0)"),
        ("claude-sonnet-4-20250514", 0.7, "Moderate creativity (temperature=0.7)"),
        ("claude-opus-4-20250514", 1.0, "Maximum creativity (temperature=1.0)"),
    ];

    for (model, temperature, desc) in configs {
        println!("{}", format!("--- {} ---", desc).cyan().bold());

        let body = json!({
            "model": model,
            "max_tokens": 512,
            "temperature": temperature,
            "messages": [
                {
                    "role": "user",
                    "content": "Describe the Rust programming language in one word"
                }
            ]
        });

        let result = common::anthropic_post(&body, &api_key).await?;

        if let Some(content) = result["content"][0]["text"].as_str() {
            println!("{} {}", "Claude:".green().bold(), content);
        }

        println!("{} {}", "Model:".dimmed(), model);
        println!();
    }

    println!(
        "{}",
        "💡 Tip: higher temperature values produce more creative but less predictable responses"
            .yellow()
    );

    Ok(())
}
