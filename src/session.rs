use std::{
    collections::BTreeMap,
    io::Error,
    path::{Path, PathBuf},
};

use serde_json::json;

use crate::{
    agent::Conversation,
    skills::{Skill, SkillLoader},
};

pub struct Session {
    cwd: PathBuf,
    pub conversation: Conversation,
    pub skills_by_name: BTreeMap<String, Skill>,
}

impl Session {
    pub fn new<L: SkillLoader>(cwd: PathBuf, skill_loader: &L) -> Result<Session, Error> {
        let mut conversation = Conversation::default();

        let skills_by_name = skill_loader.load_skills()?;

        let skill_message = format!(
            "You have access to the following skills:\n\n{}",
            skills_by_name
                .iter()
                .map(|(name, skill)| format!("- {}: {}", name, skill.description))
                .collect::<Vec<String>>()
                .join("\n")
        );

        if !skills_by_name.is_empty() {
            conversation.messages.push(json!({
                "role": "system",
                "content": skill_message
            }));
        }

        Ok(Session {
            cwd,
            conversation,
            skills_by_name,
        })
    }

    pub fn empty(cwd: PathBuf) -> Self {
        Self {
            cwd,
            conversation: Conversation::default(),
            skills_by_name: BTreeMap::new(),
        }
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }
}
