use crate::paths::Paths;

/// Settings shared by every command: where the files live and the global flags.
#[derive(Debug, Clone)]
pub struct Ctx {
    pub paths: Paths,
    /// Answer yes to every prompt.
    pub yes: bool,
    /// Skip safety prompts and override locks.
    pub force: bool,
    /// Show what would change without saving anything.
    pub dry_run: bool,
}
