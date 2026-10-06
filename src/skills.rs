use std::{
    fs,
    io::{Error, ErrorKind},
    path::Path,
};

use gray_matter::{Matter, ParsedEntity, engine::YAML};
use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq)]
pub struct Skill {
    name: String,
    description: String,
}

struct FileSystemSkillLoader {}
impl FileSystemSkillLoader {
    fn extract_skill(skills_file: &Path) -> Result<Skill, Error> {
        let input = fs::read_to_string(skills_file)?;

        let parser = Matter::<YAML>::new();
        let parsed: ParsedEntity<Skill> = parser
            .parse(&input)
            .map_err(|error| Error::new(ErrorKind::InvalidData, error))?;

        parsed.data.ok_or_else(|| {
            Error::new(
                ErrorKind::InvalidData,
                "skill file is missing YAML frontmatter",
            )
        })
    }

    fn load_skills(&self, skill_root: &Path) -> Result<Vec<Skill>, Error> {
        let mut skills = Vec::new();
        if !fs::exists(skill_root)? {
            return Ok(skills);
        }

        let entries = skill_root.read_dir()?;

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

                skills.push(skill);
            }
        }
        Ok(skills)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::skills::{FileSystemSkillLoader, Skill};

    #[test]
    fn should_load_skills_from_the_disk() {
        let loader = FileSystemSkillLoader {};
        let skill_root = Path::new("test_skills/skills");
        let skills = loader.load_skills(skill_root).expect("to read the skills");

        assert_eq!(skills.len(), 1);

        let expected = Skill {
            name: "apple".to_string(),
            description: "Deploys the apple service to production.".to_string(),
        };
        assert_eq!(expected, skills[0]);
    }
}
