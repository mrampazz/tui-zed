use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context as _, Result};

/// Status of a file relative to HEAD.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitFileStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
    Untracked,
}

/// A file with its git status.
#[derive(Debug, Clone)]
pub struct GitStatusEntry {
    pub path: PathBuf,
    pub status: GitFileStatus,
}

/// Diff hunk in a file.
#[derive(Debug, Clone)]
pub struct DiffHunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
}

/// Status of a diff hunk for gutter display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffHunkStatus {
    Added,
    Modified,
    Removed,
}

/// A lightweight wrapper around git CLI operations.
pub struct GitRepo {
    workdir: PathBuf,
}

impl GitRepo {
    /// Detect the git repository for the given path.
    /// Returns `None` if not inside a git repo.
    pub fn detect(path: &Path) -> Option<Self> {
        let output = Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .current_dir(path)
            .output()
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let workdir = String::from_utf8(output.stdout).ok()?;
        Some(Self {
            workdir: PathBuf::from(workdir.trim()),
        })
    }

    pub fn workdir(&self) -> &Path {
        &self.workdir
    }

    /// Get the current branch name.
    pub fn current_branch(&self) -> Result<String> {
        let output = self.git(&["rev-parse", "--abbrev-ref", "HEAD"])?;
        Ok(output.trim().to_string())
    }

    /// Get the status of all changed files.
    pub fn status(&self) -> Result<Vec<GitStatusEntry>> {
        let output = self.git(&["status", "--porcelain=v1"])?;
        let mut entries = Vec::new();

        for line in output.lines() {
            if line.len() < 4 {
                continue;
            }
            let index_status = line.as_bytes()[0];
            let worktree_status = line.as_bytes()[1];
            let path = &line[3..];

            let status = match (index_status, worktree_status) {
                (b'?', b'?') => GitFileStatus::Untracked,
                (b'A', _) | (_, b'A') => GitFileStatus::Added,
                (b'D', _) | (_, b'D') => GitFileStatus::Deleted,
                (b'R', _) => GitFileStatus::Renamed,
                _ => GitFileStatus::Modified,
            };

            entries.push(GitStatusEntry {
                path: PathBuf::from(path),
                status,
            });
        }

        Ok(entries)
    }

    /// Get the HEAD version of a file's contents.
    pub fn head_text(&self, path: &Path) -> Result<Option<String>> {
        let relative = path
            .strip_prefix(&self.workdir)
            .unwrap_or(path);

        let output = Command::new("git")
            .args(["show", &format!("HEAD:{}", relative.display())])
            .current_dir(&self.workdir)
            .output()
            .context("failed to run git show")?;

        if output.status.success() {
            Ok(Some(String::from_utf8_lossy(&output.stdout).into_owned()))
        } else {
            Ok(None)
        }
    }

    /// Compute diff hunks between HEAD and current buffer text.
    pub fn diff_hunks(old_text: &str, new_text: &str) -> Vec<DiffHunk> {
        use imara_diff::intern::InternedInput;
        use imara_diff::{Algorithm, UnifiedDiffBuilder, diff};

        let input = InternedInput::new(old_text, new_text);
        let diff_output = diff(
            Algorithm::Histogram,
            &input,
            UnifiedDiffBuilder::new(&input),
        );

        parse_unified_diff_hunks(&diff_output)
    }

    /// Determine the gutter status for a given line based on diff hunks.
    pub fn line_diff_status(hunks: &[DiffHunk], line: u32) -> Option<DiffHunkStatus> {
        // line is 0-indexed buffer row, hunks use 1-indexed new_start
        let line_1indexed = line + 1;

        for hunk in hunks {
            let hunk_end = hunk.new_start + hunk.new_lines;
            if line_1indexed >= hunk.new_start && line_1indexed < hunk_end {
                if hunk.old_lines == 0 {
                    return Some(DiffHunkStatus::Added);
                } else {
                    return Some(DiffHunkStatus::Modified);
                }
            }
            // Deletion marker: shown at the line just before the deletion point
            if hunk.new_lines == 0 && line_1indexed == hunk.new_start {
                return Some(DiffHunkStatus::Removed);
            }
        }
        None
    }

    fn git(&self, args: &[&str]) -> Result<String> {
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.workdir)
            .output()
            .with_context(|| format!("failed to run git {}", args.join(" ")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("git {} failed: {}", args.join(" "), stderr.trim());
        }

        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

/// Parse unified diff output into DiffHunk structs.
fn parse_unified_diff_hunks(diff: &str) -> Vec<DiffHunk> {
    let mut hunks = Vec::new();

    for line in diff.lines() {
        if let Some(header) = line.strip_prefix("@@ ") {
            if let Some(end) = header.find(" @@") {
                let ranges = &header[..end];
                if let Some((old_range, new_range)) = ranges.split_once(' ') {
                    let old = parse_hunk_range(old_range.trim_start_matches('-'));
                    let new = parse_hunk_range(new_range.trim_start_matches('+'));
                    if let (Some((old_start, old_lines)), Some((new_start, new_lines))) =
                        (old, new)
                    {
                        hunks.push(DiffHunk {
                            old_start,
                            old_lines,
                            new_start,
                            new_lines,
                        });
                    }
                }
            }
        }
    }

    hunks
}

fn parse_hunk_range(s: &str) -> Option<(u32, u32)> {
    if let Some((start, count)) = s.split_once(',') {
        Some((start.parse().ok()?, count.parse().ok()?))
    } else {
        Some((s.parse().ok()?, 1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_hunks_addition() {
        let old = "line1\nline2\nline3\n";
        let new = "line1\nline2\nnew_line\nline3\n";
        let hunks = GitRepo::diff_hunks(old, new);
        assert!(!hunks.is_empty());
    }

    #[test]
    fn test_diff_hunks_no_change() {
        let text = "hello\nworld\n";
        let hunks = GitRepo::diff_hunks(text, text);
        assert!(hunks.is_empty());
    }

    #[test]
    fn test_line_diff_status_added() {
        let hunks = vec![DiffHunk {
            old_start: 2,
            old_lines: 0,
            new_start: 3,
            new_lines: 2,
        }];
        // Lines 2 and 3 (0-indexed) should be Added (new_start=3, new_lines=2 → lines 3,4 in 1-indexed)
        assert_eq!(
            GitRepo::line_diff_status(&hunks, 2),
            Some(DiffHunkStatus::Added)
        );
        assert_eq!(
            GitRepo::line_diff_status(&hunks, 3),
            Some(DiffHunkStatus::Added)
        );
        assert_eq!(GitRepo::line_diff_status(&hunks, 0), None);
    }

    #[test]
    fn test_parse_hunk_range() {
        assert_eq!(parse_hunk_range("1,3"), Some((1, 3)));
        assert_eq!(parse_hunk_range("5"), Some((5, 1)));
    }
}
