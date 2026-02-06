// Example: Default Auto-Enable DynamicSkill

use aagt_core::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize provider
    let provider = OpenAIProvider::new(
        std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY not set")
    );

    // ✅ DEFAULT BEHAVIOR: DynamicSkill auto-enabled
    // No need to manually call .with_dynamic_skills()
    let agent = Agent::builder(provider)
        .model("gpt-4o")
        .system_prompt("You are a helpful trading assistant.")
        .build()?;  // ← DynamicSkill automatically loaded from ./skills
    
    // The agent now has:
    // 1. All skills from ./skills/ directory (if exists)
    // 2. ClawHub tool (for installing new skills)
    // 3. ReadSkillDoc tool (for reading skill documentation)
    // 4. Python Sidecar: DISABLED (secure default)

    println!("Agent built successfully with default DynamicSkill support!");
    
    Ok(())
}

// Output when ./skills exists:
// INFO: No execution model configured. Auto-enabling DynamicSkill (default)...
// INFO: Loaded DynamicSkills from ./skills

// Output when ./skills doesn't exist:
// INFO: No execution model configured. Auto-enabling DynamicSkill (default)...
// INFO: DynamicSkill auto-enable skipped (no skills found): ...
// (Agent continues to function normally)
