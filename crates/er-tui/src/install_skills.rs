//! Install Easy Review agent skills (MCP companion).
//!
//! Prefers `gh skill install --from-local` when available (same placement as
//! `gh skill install github/gh-stack`). Falls back to copying the embedded
//! `skills/er-review/SKILL.md` into well-known agent skill directories.

use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Bundled skill — kept in sync with repo `skills/er-review/SKILL.md`.
const ER_REVIEW_SKILL_MD: &str = include_str!("../../../skills/er-review/SKILL.md");

const SKILL_NAME: &str = "er-review";

#[derive(Debug, Clone)]
pub struct InstallSkillsOpts {
    pub agents: Vec<String>,
    pub scope: Scope,
    pub force: bool,
    pub dir: Option<PathBuf>,
    /// When true, try `gh skill install` before direct copy.
    pub prefer_gh: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    User,
    Project,
}

impl Scope {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "user" => Ok(Self::User),
            "project" => Ok(Self::Project),
            other => bail!("invalid --scope {other:?}; expected user or project"),
        }
    }
}

pub fn run(opts: InstallSkillsOpts) -> Result<()> {
    if let Some(dir) = &opts.dir {
        install_to_dir(dir, opts.force)?;
        println!(
            "Installed {SKILL_NAME} → {}",
            dir.join(SKILL_NAME).display()
        );
        return Ok(());
    }

    if opts.agents.is_empty() {
        bail!("pass at least one --agent");
    }

    let mut installed = Vec::new();
    let mut errors = Vec::new();

    for agent in &opts.agents {
        match install_for_agent(agent, opts.scope, opts.force, opts.prefer_gh) {
            Ok(dest) => {
                println!(
                    "Installed {SKILL_NAME} ({agent}, {:?}) → {}",
                    opts.scope,
                    dest.display()
                );
                installed.push(dest);
            }
            Err(e) => {
                eprintln!("error: {agent}: {e:#}");
                errors.push(agent.clone());
            }
        }
    }

    if installed.is_empty() {
        bail!("failed to install skill for any agent");
    }
    if !errors.is_empty() {
        bail!("partial failure for agents: {}", errors.join(", "));
    }

    println!(
        "\nSay \"ER review\" in your agent (with easy-review MCP connected) to run prepare → author → upload."
    );
    Ok(())
}

fn install_for_agent(agent: &str, scope: Scope, force: bool, prefer_gh: bool) -> Result<PathBuf> {
    let dest_root = skills_root(agent, scope)?;
    let dest_skill = dest_root.join(SKILL_NAME);

    if prefer_gh && gh_skill_available() {
        match install_via_gh(agent, scope, force) {
            Ok(dest) => return Ok(dest),
            Err(e) => {
                eprintln!("note: gh skill install failed ({e:#}); copying skill directly");
            }
        }
    }

    install_to_dir(&dest_root, force)?;
    Ok(dest_skill)
}

fn install_via_gh(agent: &str, scope: Scope, force: bool) -> Result<PathBuf> {
    let staging = staging_dir()?;
    let scope_str = match scope {
        Scope::User => "user",
        Scope::Project => "project",
    };

    let mut cmd = Command::new("gh");
    cmd.args([
        "skill",
        "install",
        staging.to_str().context("staging path")?,
        SKILL_NAME,
        "--from-local",
        "--agent",
        agent,
        "--scope",
        scope_str,
    ]);
    if force {
        cmd.arg("--force");
    }

    let out = cmd.output().context("run gh skill install")?;
    let _ = fs::remove_dir_all(&staging);
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        bail!("gh skill install failed: {stderr}{stdout}");
    }

    // Best-effort: report the directory we would have used for direct install.
    Ok(skills_root(agent, scope)?.join(SKILL_NAME))
}

fn install_to_dir(skills_root: &Path, force: bool) -> Result<()> {
    let dest = skills_root.join(SKILL_NAME);
    let skill_md = dest.join("SKILL.md");

    if dest.exists() {
        if !force {
            bail!(
                "{} already exists (pass --force to overwrite)",
                dest.display()
            );
        }
        fs::remove_dir_all(&dest).with_context(|| format!("remove existing {}", dest.display()))?;
    }

    fs::create_dir_all(&dest).with_context(|| format!("mkdir {}", dest.display()))?;
    fs::write(&skill_md, ER_REVIEW_SKILL_MD)
        .with_context(|| format!("write {}", skill_md.display()))?;
    Ok(())
}

fn staging_dir() -> Result<PathBuf> {
    let base = std::env::temp_dir().join(format!("er-skills-{}", std::process::id()));
    let skill_dir = base.join("skills").join(SKILL_NAME);
    fs::create_dir_all(&skill_dir).context("mkdir staging skills dir")?;
    fs::write(skill_dir.join("SKILL.md"), ER_REVIEW_SKILL_MD).context("write staging SKILL.md")?;
    Ok(base)
}

fn gh_skill_available() -> bool {
    Command::new("gh")
        .args(["skill", "--help"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn skills_root(agent: &str, scope: Scope) -> Result<PathBuf> {
    match scope {
        Scope::User => user_skills_root(agent),
        Scope::Project => Ok(project_skills_root(agent)),
    }
}

fn user_skills_root(agent: &str) -> Result<PathBuf> {
    let home = dirs::home_dir().context("HOME not set")?;
    let rel = match agent {
        "cursor" => ".cursor/skills",
        "claude-code" => ".claude/skills",
        "codex" => ".codex/skills",
        "github-copilot" => ".copilot/skills",
        "universal" => ".config/agents/skills",
        "gemini-cli" => ".gemini/skills",
        "windsurf" => ".windsurf/skills",
        other => bail!(
            "unsupported --agent {other:?}; try cursor, claude-code, codex, github-copilot, universal, or --dir"
        ),
    };
    Ok(home.join(rel))
}

fn project_skills_root(agent: &str) -> PathBuf {
    // Matches `gh skill install` conventions: many hosts share `.agents/skills`
    // at project scope; Claude Code uses `.claude/skills`.
    match agent {
        "claude-code" => PathBuf::from(".claude/skills"),
        _ => PathBuf::from(".agents/skills"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_skill_has_frontmatter() {
        assert!(ER_REVIEW_SKILL_MD.contains("name: er-review"));
        assert!(ER_REVIEW_SKILL_MD.contains("prepare_review"));
        assert!(ER_REVIEW_SKILL_MD.contains("upload_artifacts"));
    }

    #[test]
    fn install_to_dir_writes_skill() {
        let tmp = tempfile_dir();
        install_to_dir(&tmp, false).unwrap();
        let md = fs::read_to_string(tmp.join(SKILL_NAME).join("SKILL.md")).unwrap();
        assert!(md.contains("ER review"));
        // second install without force fails
        assert!(install_to_dir(&tmp, false).is_err());
        install_to_dir(&tmp, true).unwrap();
    }

    fn tempfile_dir() -> PathBuf {
        let p = std::env::temp_dir().join(format!("er-skill-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }
}
