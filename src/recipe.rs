//! Recipes: named bundles of commands that run together, each in its own run
//! tab. Bundled ones come from `default_recipes.toml`; users add or override
//! them in `~/.config/pivt/recipes.toml` (same schema, same name replaces).
//!
//! A recipe "matches" the current box when one of its ports is in PORTS or one
//! of its services was seen by the last parsed scan (`P`), so the picker can
//! float the relevant ones to the top and `A` can run all of them.

use crate::model::config_dir;
use serde::Deserialize;

const BUNDLED: &str = include_str!("default_recipes.toml");

#[derive(Debug, Clone, Deserialize)]
pub struct Recipe {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub ports: Vec<u16>,
    #[serde(default)]
    pub services: Vec<String>,
    /// Command keys, `category/name`.
    pub steps: Vec<String>,
}

#[derive(Deserialize)]
struct RecipeFile {
    #[serde(default, rename = "recipe")]
    recipes: Vec<Recipe>,
}

impl Recipe {
    /// Unconditional recipes (no ports/services) always apply.
    pub fn is_generic(&self) -> bool {
        self.ports.is_empty() && self.services.is_empty()
    }

    /// Does this recipe apply to a box with these open ports / services?
    pub fn matches(&self, ports: &[u16], services: &[String]) -> bool {
        self.ports.iter().any(|p| ports.contains(p))
            || self
                .services
                .iter()
                .any(|s| services.iter().any(|seen| seen.eq_ignore_ascii_case(s)))
    }
}

fn parse(text: &str) -> Vec<Recipe> {
    toml::from_str::<RecipeFile>(text).map(|f| f.recipes).unwrap_or_default()
}

/// Bundled recipes, then the user's file layered on top (same name replaces).
pub fn load_all() -> Vec<Recipe> {
    let mut out = parse(BUNDLED);
    let user = std::fs::read_to_string(config_dir().join("recipes.toml")).unwrap_or_default();
    for r in parse(&user) {
        match out.iter_mut().find(|x| x.name == r.name) {
            Some(slot) => *slot = r,
            None => out.push(r),
        }
    }
    out
}

/// Parse a PORTS var like "22,80,445" into numbers.
pub fn parse_ports(s: &str) -> Vec<u16> {
    s.split(|c: char| c == ',' || c.is_whitespace())
        .filter_map(|p| p.trim().parse().ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_recipes_reference_real_commands() {
        let recipes = parse(BUNDLED);
        assert!(recipes.len() >= 5);
        let cmds = crate::model::load_bundled_for_tests();
        for r in &recipes {
            for step in &r.steps {
                assert!(
                    cmds.iter().any(|c| format!("{}/{}", c.category, c.name) == *step),
                    "recipe {:?} step {step:?} is not a bundled command",
                    r.name
                );
            }
        }
    }

    #[test]
    fn matching() {
        let smb = parse(BUNDLED).into_iter().find(|r| r.name == "smb enum").unwrap();
        assert!(smb.matches(&parse_ports("22, 445"), &[]));
        assert!(smb.matches(&[], &["microsoft-ds".into()]));
        assert!(!smb.matches(&[22, 80], &["ssh".into()]));
    }
}
