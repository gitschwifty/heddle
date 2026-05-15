//! Command registry with lookup + suggestion.

use std::collections::HashMap;

use crate::tools::string_distance::find_closest;

use super::types::SlashCommand;

#[derive(Default)]
pub struct CommandRegistry {
    commands: HashMap<String, SlashCommand>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn register(&mut self, command: SlashCommand) {
        self.commands.insert(command.name.clone(), command);
    }
    pub fn get(&self, name: &str) -> Option<&SlashCommand> {
        self.commands.get(name)
    }
    pub fn all(&self) -> Vec<&SlashCommand> {
        self.commands.values().collect()
    }
    pub fn suggest(&self, name: &str) -> Option<String> {
        let candidates: Vec<String> = self.commands.keys().cloned().collect();
        find_closest(name, &candidates, 3).map(String::from)
    }
}
