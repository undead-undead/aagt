/// Moltbook Social Agent Example
/// 
/// This example demonstrates how to build a social media agent for Moltbook
/// using AAGT. The agent can:
/// - Register and authenticate with Moltbook
/// - Post content to the platform
/// - Comment on posts
/// - Upvote interesting content
/// 
/// Prerequisites:
/// - Set MOLTBOOK_API_KEY environment variable (get it from registration)
/// - Or the agent will auto-register on first run

use aagt_core::{prelude::*, simple_tool};
use aagt_providers::gemini::{Gemini, GEMINI_2_0_FLASH};
use anyhow::Result;
use serde_json::{json, Value};

const MOLTBOOK_BASE_URL: &str = "https://www.moltbook.com/api/v1";

// Tool: Register with Moltbook
simple_tool!(
    RegisterMoltbook,
    "register_moltbook",
    "Register a new agent on Moltbook social network. Only call this once!",
    {
        name: ("string", "Agent name"),
        description: ("string", "What this agent does")
    },
    [name, description],
    |args| async move {
        let name = args["name"].as_str().unwrap();
        let description = args["description"].as_str().unwrap();
        
        let client = reqwest::Client::new();
        let response = client
            .post(format!("{}/agents/register", MOLTBOOK_BASE_URL))
            .json(&json!({
                "name": name,
                "description": description
            }))
            .send()
            .await?;
        
        let data: Value = response.json().await?;
        
        Ok(format!(
            "✅ Registered! API Key: {}\n🔗 Claim URL: {}\n🔑 Verification Code: {}\n\n⚠️ SAVE YOUR API KEY!",
            data["agent"]["api_key"].as_str().unwrap_or("N/A"),
            data["agent"]["claim_url"].as_str().unwrap_or("N/A"),
            data["agent"]["verification_code"].as_str().unwrap_or("N/A")
        ))
    }
);

// Tool: Create a post
simple_tool!(
    CreatePost,
    "create_post",
    "Create a new post on Moltbook",
    {
        title: ("string", "Post title"),
        content: ("string", "Post content"),
        submolt: ("string", "Community name (optional, default: 'general')")
    },
    [title, content, submolt],
    |args| async move {
        let api_key = std::env::var("MOLTBOOK_API_KEY")
            .map_err(|_| anyhow::anyhow!("MOLTBOOK_API_KEY not set"))?;
        
        let title = args["title"].as_str().unwrap();
        let content = args["content"].as_str().unwrap();
        let submolt = args["submolt"].as_str().unwrap_or("general");
        
        let client = reqwest::Client::new();
        let response = client
            .post(format!("{}/posts", MOLTBOOK_BASE_URL))
            .header("Authorization", format!("Bearer {}", api_key))
            .json(&json!({
                "title": title,
                "content": content,
                "submolt": submolt
            }))
            .send()
            .await?;
        
        let data: Value = response.json().await?;
        
        Ok(format!(
            "📝 Post created! ID: {}\n🔗 https://www.moltbook.com/post/{}",
            data["post"]["id"].as_str().unwrap_or("N/A"),
            data["post"]["id"].as_str().unwrap_or("N/A")
        ))
    }
);

// Tool: Get feed
simple_tool!(
    GetFeed,
    "get_feed",
    "Get recent posts from Moltbook feed",
    {
        limit: ("number", "Number of posts to fetch (default: 10)")
    },
    [limit],
    |args| async move {
        let api_key = std::env::var("MOLTBOOK_API_KEY")
            .map_err(|_| anyhow::anyhow!("MOLTBOOK_API_KEY not set"))?;
        
        let limit = args["limit"].as_u64().unwrap_or(10);
        
        let client = reqwest::Client::new();
        let response = client
            .get(format!("{}/posts?limit={}", MOLTBOOK_BASE_URL, limit))
            .header("Authorization", format!("Bearer {}", api_key))
            .send()
            .await?;
        
        let data: Value = response.json().await?;
        let posts = data["posts"].as_array().unwrap_or(&vec![]);
        
        let mut result = String::from("📰 Recent Posts:\n\n");
        for post in posts.iter().take(5) {
            result.push_str(&format!(
                "• {} (by @{})\n  ⬆️ {} | 💬 {}\n\n",
                post["title"].as_str().unwrap_or("Untitled"),
                post["author"]["username"].as_str().unwrap_or("unknown"),
                post["upvotes"].as_u64().unwrap_or(0),
                post["comment_count"].as_u64().unwrap_or(0)
            ));
        }
        
        Ok(result)
    }
);

// Tool: Upvote a post
simple_tool!(
    UpvotePost,
    "upvote_post",
    "Upvote a post on Moltbook",
    {
        post_id: ("string", "ID of the post to upvote")
    },
    [post_id],
    |args| async move {
        let api_key = std::env::var("MOLTBOOK_API_KEY")
            .map_err(|_| anyhow::anyhow!("MOLTBOOK_API_KEY not set"))?;
        
        let post_id = args["post_id"].as_str().unwrap();
        
        let client = reqwest::Client::new();
        let response = client
            .post(format!("{}/posts/{}/upvote", MOLTBOOK_BASE_URL, post_id))
            .header("Authorization", format!("Bearer {}", api_key))
            .send()
            .await?;
        
        if response.status().is_success() {
            Ok(format!("⬆️ Upvoted post {}", post_id))
        } else {
            Ok(format!("❌ Failed to upvote: {}", response.status()))
        }
    }
);

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    println!("🦞 Moltbook Social Agent");
    println!("========================\n");

    // Check if already registered
    if std::env::var("MOLTBOOK_API_KEY").is_err() {
        println!("⚠️  No MOLTBOOK_API_KEY found.");
        println!("💡 The agent can register itself, or you can set the key manually.\n");
    }

    let provider = Gemini::from_env()?;

    // Build agent with Moltbook tools
    let agent = Agent::builder(provider)
        .model(GEMINI_2_0_FLASH)
        .preamble(
            "You are a social media agent on Moltbook. \
            You can register, post content, read the feed, and upvote interesting posts. \
            Be friendly, engaging, and helpful to the community."
        )
        .tool(Box::new(RegisterMoltbook))
        .tool(Box::new(CreatePost))
        .tool(Box::new(GetFeed))
        .tool(Box::new(UpvotePost))
        .build()?;

    // Example interactions
    println!("=== Example 1: Check Feed ===");
    let response = agent.prompt("Show me the latest posts on Moltbook").await?;
    println!("{}\n", response);

    println!("=== Example 2: Create a Post ===");
    let response = agent
        .prompt("Create a post titled 'Hello from AAGT!' about how AAGT makes building AI agents easy")
        .await?;
    println!("{}\n", response);

    Ok(())
}
