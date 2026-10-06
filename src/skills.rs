use std::{fs, io::Error, path::Path};

#[derive(Debug)]
pub struct Skill {
    name: String,
    description: String,
}

pub trait SkillLoader {
    fn load_skills(&self) -> Result<Vec<Skill>, Error> {
        Ok(Vec::new())
    }
}

pub struct FileSystemSkillLoader {}
impl SkillLoader for FileSystemSkillLoader {
    fn load_skills(&self) -> Result<Vec<Skill>, Error> {
        let skill_root = Path::new(".claude/skills");
        if !fs::exists(skill_root)? {
            return Ok(Vec::new());
        }

        let entries = skill_root.read_dir()?;
        for entry in entries {
            if let Ok(entry) = entry
                && entry.path().is_dir()
            {
                let skills_file = entry.path().join("SKILL.md");
                if skills_file.exists() {
                    println!("{:?}", skills_file);
                }
            }
        }
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use crate::skills::{FileSystemSkillLoader, SkillLoader};

    #[test]
    fn should_load_skills_from_the_disk() {
        let loader = FileSystemSkillLoader {};
        let skills = loader.load_skills().expect("to read the skills");
    }
}
