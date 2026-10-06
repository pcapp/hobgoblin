use std::{
    collections::HashMap,
    fs,
    io::{Error, ErrorKind},
    path::{Path, PathBuf},
};

use gray_matter::{Matter, ParsedEntity, engine::YAML};
use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
struct FrontMatter {
    pub name: String,
    pub description: String,
}

pub trait SkillLoader {
    fn load_skills(&self) -> Result<HashMap<String, Skill>, Error>;
}

pub struct FileSystemSkillLoader {
    pub skill_root: PathBuf,
}

impl FileSystemSkillLoader {
    fn extract_skill(skills_file: &Path) -> Result<Skill, Error> {
        let input = fs::read_to_string(skills_file)?;

        let parser = Matter::<YAML>::new();
        let parsed: ParsedEntity<FrontMatter> = parser
            .parse(&input)
            .map_err(|error| Error::new(ErrorKind::InvalidData, error))?;

        let front_matter = parsed.data.ok_or_else(|| {
            Error::new(
                ErrorKind::InvalidData,
                "skill file is missing YAML frontmatter",
            )
        })?;

        Ok(Skill {
            name: front_matter.name,
            description: front_matter.description,
            content: parsed.content,
        })
    }
}

impl SkillLoader for FileSystemSkillLoader {
    fn load_skills(&self) -> Result<HashMap<String, Skill>, Error> {
        let mut skills_by_name: HashMap<String, Skill> = HashMap::new();
        if !fs::exists(&self.skill_root)? {
            return Ok(skills_by_name);
        }

        let entries = self.skill_root.read_dir()?;

        for entry in entries {
            if let Ok(entry) = entry
                && entry.path().is_dir()
            {
                let skills_file = entry.path().join("SKILL.md");
                let skill = match FileSystemSkillLoader::extract_skill(&skills_file) {
                    Err(_) => {
                        continue;
                    }
                    Ok(skill) => skill,
                };

                skills_by_name.insert(skill.name.clone(), skill);
            }
        }
        Ok(skills_by_name)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::skills::{FileSystemSkillLoader, Skill, SkillLoader};

    #[test]
    fn should_load_skills_from_the_disk() {
        let loader = FileSystemSkillLoader {
            skill_root: PathBuf::from("test_skills/skills"),
        };

        let skills_by_name = loader.load_skills().expect("to read the skills");

        assert_eq!(skills_by_name.len(), 1);

        let expected = Skill {
            name: "apple".to_string(),
            description: "Deploys the apple service to production.".to_string(),
            content: "Run the deploy script and report the version that was deployed.".to_string(),
        };
        assert_eq!(expected, *skills_by_name.get("apple").unwrap());
    }
}
