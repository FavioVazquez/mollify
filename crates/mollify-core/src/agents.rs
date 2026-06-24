//! Agent-integration installer.
//!
//! Mollify ships ready-to-commit skills, rules, hooks, slash-commands, and
//! workflows for several coding agents. Those artifacts live in this repo
//! (`.claude/`, `.cursor/`, `.gemini/`, `.codex/`, `.agents/`, `.devin/`,
//! `.windsurf/`, plus a few root marker files). This module embeds them into
//! the binary via [`include_dir`] so `mollify init --agent <name>` can scaffold
//! the right set into any project — regardless of whether mollify was installed
//! through `uv`, `pip`, `cargo`, or `npm`. The embedded copy is version-matched
//! to the CLI by construction (it is compiled from the same tree).
//!
//! Existing files are never overwritten unless `force` is set; the installer
//! reports created / skipped counts so a human stays in control of their repo.

use camino::{Utf8Path, Utf8PathBuf};
use include_dir::{include_dir, Dir, File};

/// Every agent artifact, mirrored into the crate by `scripts/sync-agent-assets.sh`.
///
/// Embedding from *inside* the crate (rather than reaching out to `../../`)
/// keeps the published crate self-contained, so `cargo install mollify-cli`
/// (crates.io) builds identically to the maturin/npm/source builds. Each path
/// within this tree already equals its install-relative destination (e.g.
/// `.claude/skills/mollify/SKILL.md`). The `assets_match_repo_root_sources`
/// test guards against the mirror drifting from the canonical sources.
static ASSETS: Dir = include_dir!("$CARGO_MANIFEST_DIR/assets");

/// A coding agent we can scaffold integration files for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agent {
    /// Claude Code: `.mcp.json`, `CLAUDE.md`, `.claude/` (skills, commands, hooks).
    Claude,
    /// Cursor: `.cursor/` (rules, MCP config, slash commands).
    Cursor,
    /// Gemini CLI: `GEMINI.md`, `.gemini/` (settings + commands).
    Gemini,
    /// Codex / portable open-standard: `AGENTS.md`, `.codex/`, `.agents/`.
    Codex,
    /// Devin Desktop / Windsurf Cascade: `.devin/` + `.windsurf/`.
    Cascade,
}

impl Agent {
    /// All agents, for `--agent all`.
    pub const ALL: [Agent; 5] = [
        Agent::Claude,
        Agent::Cursor,
        Agent::Gemini,
        Agent::Codex,
        Agent::Cascade,
    ];

    /// Parse an agent name (case-insensitive). Accepts a few friendly aliases.
    pub fn parse(name: &str) -> Option<Agent> {
        match name.to_ascii_lowercase().as_str() {
            "claude" | "claude-code" => Some(Agent::Claude),
            "cursor" => Some(Agent::Cursor),
            "gemini" | "gemini-cli" => Some(Agent::Gemini),
            "codex" | "agents" => Some(Agent::Codex),
            "cascade" | "devin" | "windsurf" => Some(Agent::Cascade),
            _ => None,
        }
    }

    /// The canonical name used in messages.
    pub fn name(self) -> &'static str {
        match self {
            Agent::Claude => "claude",
            Agent::Cursor => "cursor",
            Agent::Gemini => "gemini",
            Agent::Codex => "codex",
            Agent::Cascade => "cascade",
        }
    }

    /// Top-level entries (within [`ASSETS`]) this agent installs. Each is either
    /// a directory (installed recursively) or a single root marker file.
    fn entries(self) -> &'static [&'static str] {
        match self {
            // Claude and Cascade install hooks that invoke the advisory report
            // helper (`scripts/mollify-report.sh`), so they ship it too.
            Agent::Claude => &[
                ".claude",
                ".mcp.json",
                "CLAUDE.md",
                "scripts/mollify-report.sh",
            ],
            Agent::Cursor => &[".cursor"],
            Agent::Gemini => &[".gemini", "GEMINI.md"],
            Agent::Codex => &[".codex", ".agents", "AGENTS.md"],
            Agent::Cascade => &[".devin", ".windsurf", "scripts/mollify-report.sh"],
        }
    }

    /// The (destination-relative-path, contents) pairs this agent installs.
    /// An embedded path is already relative to the install root, so it doubles
    /// as the destination.
    fn artifacts(self) -> Vec<(Utf8PathBuf, &'static [u8])> {
        let mut out: Vec<(Utf8PathBuf, &'static [u8])> = Vec::new();
        for name in self.entries() {
            if let Some(file) = ASSETS.get_file(name) {
                out.push((path_of(file), file.contents()));
            } else if let Some(dir) = ASSETS.get_dir(name) {
                let mut files: Vec<&File> = Vec::new();
                collect_files(dir, &mut files);
                for f in files {
                    out.push((path_of(f), f.contents()));
                }
            } else {
                panic!("embedded asset `{name}` is missing — run scripts/sync-agent-assets.sh");
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }
}

/// An embedded file's path as UTF-8 (already relative to the install root).
fn path_of(f: &File) -> Utf8PathBuf {
    Utf8Path::from_path(f.path())
        .expect("embedded asset paths are valid UTF-8")
        .to_path_buf()
}

/// Recursively gather every embedded `File` under `dir`.
fn collect_files<'a>(dir: &'a Dir<'a>, out: &mut Vec<&'a File<'a>>) {
    for entry in dir.entries() {
        match entry {
            include_dir::DirEntry::File(f) => out.push(f),
            include_dir::DirEntry::Dir(d) => collect_files(d, out),
        }
    }
}

/// What happened to a single artifact during install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileOutcome {
    Created,
    Overwritten,
    Skipped,
}

/// One installed (or skipped) artifact.
#[derive(Debug, Clone)]
pub struct InstalledFile {
    pub path: Utf8PathBuf,
    pub outcome: FileOutcome,
}

/// Scaffold `agent`'s artifacts under `root`. Existing files are skipped unless
/// `force` is true. Returns a per-file outcome list (deterministic order).
pub fn install(root: &Utf8Path, agent: Agent, force: bool) -> std::io::Result<Vec<InstalledFile>> {
    let mut results = Vec::new();
    for (rel, contents) in agent.artifacts() {
        let dest = root.join(&rel);
        let exists = dest.exists();
        if exists && !force {
            results.push(InstalledFile {
                path: rel,
                outcome: FileOutcome::Skipped,
            });
            continue;
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&dest, contents)?;
        results.push(InstalledFile {
            path: rel,
            outcome: if exists {
                FileOutcome::Overwritten
            } else {
                FileOutcome::Created
            },
        });
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> Utf8PathBuf {
        let base =
            std::env::temp_dir().join(format!("mollify-agents-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        Utf8PathBuf::from_path_buf(base).unwrap()
    }

    #[test]
    fn parses_agent_names_and_aliases() {
        assert_eq!(Agent::parse("Claude"), Some(Agent::Claude));
        assert_eq!(Agent::parse("devin"), Some(Agent::Cascade));
        assert_eq!(Agent::parse("windsurf"), Some(Agent::Cascade));
        assert_eq!(Agent::parse("nope"), None);
    }

    #[test]
    fn every_agent_has_artifacts() {
        for a in Agent::ALL {
            assert!(!a.artifacts().is_empty(), "{} has no artifacts", a.name());
        }
    }

    #[test]
    fn installs_cursor_files_and_skips_existing() {
        let d = temp("cursor");
        let r = install(&d, Agent::Cursor, false).unwrap();
        assert!(r.iter().all(|f| f.outcome == FileOutcome::Created));
        // The Cursor rule file is a known artifact.
        assert!(
            d.join(".cursor/rules/mollify.mdc").exists(),
            "cursor rule not written"
        );
        // Re-running without force skips everything.
        let r2 = install(&d, Agent::Cursor, false).unwrap();
        assert!(r2.iter().all(|f| f.outcome == FileOutcome::Skipped));
        // With force, files are overwritten.
        let r3 = install(&d, Agent::Cursor, true).unwrap();
        assert!(r3.iter().all(|f| f.outcome == FileOutcome::Overwritten));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn claude_installs_root_markers() {
        let d = temp("claude");
        install(&d, Agent::Claude, false).unwrap();
        assert!(d.join(".mcp.json").exists());
        assert!(d.join(".claude/skills/mollify/SKILL.md").exists());
        std::fs::remove_dir_all(&d).ok();
    }

    /// Drift guard: the in-crate embedded mirror must byte-match the canonical
    /// repo-root sources. If this fails, run `scripts/sync-agent-assets.sh` and
    /// commit. (Dev/CI-only — reaches out to the workspace root, which exists
    /// when our own tests run, never on a crates.io consumer's build.)
    #[test]
    fn assets_match_repo_root_sources() {
        let root = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..");
        // Skip when the canonical sources aren't present (e.g. `cargo test` on a
        // published crate, where only the in-crate `assets/` mirror exists).
        if !root.join(".claude").exists() {
            return;
        }
        let mut files: Vec<&File> = Vec::new();
        collect_files(&ASSETS, &mut files);
        assert!(
            !files.is_empty(),
            "no embedded assets — sync script not run?"
        );
        for f in files {
            let rel = path_of(f);
            let src = root.join(&rel);
            let on_disk = std::fs::read(src.as_std_path()).unwrap_or_else(|_| {
                panic!("canonical source missing for {rel}; run scripts/sync-agent-assets.sh")
            });
            assert!(
                on_disk == f.contents(),
                "{rel} is out of sync; run scripts/sync-agent-assets.sh"
            );
        }
    }
}
