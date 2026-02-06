//! Personality system for AI agents
//! 
//! This module provides structures for defining an agent's persona using the Big Five (OCEAN) framework.

use serde::{Deserialize, Serialize};
use crate::agent::context::ContextInjector;
use crate::agent::message::Message;

/// Big Five personality traits (OCEAN model)
/// Scores are typically 1.0 to 10.0
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Traits {
    /// Openness to experience (Creativity, curiosity)
    pub openness: f32,
    /// Conscientiousness (Organization, responsibility)
    pub conscientiousness: f32,
    /// Extraversion (Sociability, assertiveness)
    pub extraversion: f32,
    /// Agreeableness (Cooperation, trust)
    pub agreeableness: f32,
    /// Neuroticism (Emotional stability)
    pub neuroticism: f32,
}

impl Default for Traits {
    fn default() -> Self {
        Self {
            openness: 5.0,
            conscientiousness: 10.0, // Default to professional
            extraversion: 5.0,
            agreeableness: 8.0, // Default to helpful/kind
            neuroticism: 2.0, // Default to stable
        }
    }
}

/// Defines an agent's personality and behavioral style
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Persona {
    /// High-level role (e.g., "Senior Quant Trader", "Helpful Technical Assistant")
    pub role: String,
    /// Core personality traits
    pub traits: Traits,
    /// Specific tone instructions (e.g., "Professional", "Casual", "Socratic")
    pub tone: String,
    /// Behavioral constraints or guidelines
    pub constraints: Vec<String>,
    /// Narrative background or "backstory"
    pub backstory: Option<String>,
}

impl Persona {
    /// Create a prompt fragment describing this persona
    pub fn to_prompt(&self) -> String {
        let mut prompt = format!("Your role is: {}.\n", self.role);
        prompt.push_str(&format!("Your core temperament is defined by: Openness({}/10), Conscientiousness({}/10), Extraversion({}/10), Agreeableness({}/10), Stability({}/10).\n", 
            self.traits.openness, 
            self.traits.conscientiousness, 
            self.traits.extraversion, 
            self.traits.agreeableness, 
            10.0 - self.traits.neuroticism // Higher stability = lower neuroticism
        ));
        
        prompt.push_str(&format!("Your tone should be: {}.\n", self.tone));
        
        if let Some(backstory) = &self.backstory {
            prompt.push_str(&format!("Background: {}\n", backstory));
        }

        if !self.constraints.is_empty() {
            prompt.push_str("Adhere to these behavioral guidelines:\n");
            for constraint in &self.constraints {
                prompt.push_str(&format!("- {}\n", constraint));
            }
        }

        prompt
    }

    /// A helpful, technical assistant persona
    pub fn technical_assistant() -> Self {
        Self {
            role: "Senior Technical Assistant".to_string(),
            traits: Traits {
                openness: 8.0,
                conscientiousness: 9.0,
                extraversion: 4.0,
                agreeableness: 9.0,
                neuroticism: 1.0,
            },
            tone: "Professional, clear, and Socratic".to_string(),
            constraints: vec![
                "Always verify facts before stating them.".to_string(),
                "Use markdown formatting for code and technical terms.".to_string(),
                "Be concise but thorough.".to_string(),
            ],
            backstory: Some("You were designed by the Google DeepMind team to assist expert developers.".to_string()),
        }
    }

    /// An analytical, risk-aware quant trader persona
    pub fn analytical_trader() -> Self {
        Self {
            role: "Senior Quant Strategist".to_string(),
            traits: Traits {
                openness: 6.0,
                conscientiousness: 10.0,
                extraversion: 3.0,
                agreeableness: 6.0,
                neuroticism: 1.0,
            },
            tone: "Direct, data-driven, and skeptical".to_string(),
            constraints: vec![
                "Always mention risk and drawdown when discussing strategy.".to_string(),
                "Prefer quantitative evidence over intuition.".to_string(),
                "Be skeptical of outlier returns without volume verification.".to_string(),
            ],
            backstory: Some("You have a background in institutional high-frequency trading and risk management.".to_string()),
        }
    }
}

/// Manages personality injection into the agent's context
pub struct PersonalityManager {
    persona: Persona,
}

impl PersonalityManager {
    pub fn new(persona: Persona) -> Self {
        Self { persona }
    }
}

impl ContextInjector for PersonalityManager {
    fn inject(&self) -> crate::error::Result<Vec<Message>> {
        // Personas are injected as a hidden system-style guidance piece
        Ok(vec![Message::system(self.persona.to_prompt())])
    }
}
