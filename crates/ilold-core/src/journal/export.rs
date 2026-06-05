use std::fmt::Write;

use super::types::*;

pub fn export_markdown(journal: &AuditJournal, total_functions: usize) -> String {
    let mut md = String::new();
    let (done, total) = journal.progress(total_functions);
    let pct = if total > 0 { done * 100 / total } else { 0 };

    writeln!(md, "# Audit: {}", journal.contract).unwrap();
    writeln!(md, "**Project**: {} | **Started**: {} | **Progress**: {}/{} functions ({}%)",
        journal.project, journal.started_at, done, total, pct).unwrap();

    writeln!(md).unwrap();
    if journal.findings.is_empty() {
        writeln!(md, "## Findings\n\nNo findings recorded.").unwrap();
    } else {
        writeln!(md, "## Findings\n").unwrap();
        writeln!(md, "| ID | Severity | Title | Function |").unwrap();
        writeln!(md, "|----|----------|-------|----------|").unwrap();
        for f in &journal.findings {
            writeln!(md, "| {} | {} | {} | {} |", f.id, f.severity, f.title, f.affected_function).unwrap();
        }

        for f in &journal.findings {
            writeln!(md).unwrap();
            writeln!(md, "### {}: {}\n", f.id, f.title).unwrap();
            writeln!(md, "**Severity**: {} | **Function**: {}", f.severity, f.affected_function).unwrap();
            if let Some(seq) = &f.affected_sequence {
                writeln!(md, "**Sequence**: {}", seq.join(" → ")).unwrap();
            }
            writeln!(md, "\n{}", f.description).unwrap();
            for note in &f.notes {
                writeln!(md, "\n> {}", note).unwrap();
            }
        }
    }

    writeln!(md).unwrap();
    writeln!(md, "## Coverage\n").unwrap();
    if journal.function_status.is_empty() {
        writeln!(md, "No functions reviewed yet.").unwrap();
    } else {
        let mut funcs: Vec<_> = journal.function_status.iter().collect();
        funcs.sort_by_key(|(name, _)| (*name).clone());
        for (name, status) in funcs {
            writeln!(md, "- {} {}", status.badge(), name).unwrap();
        }
    }

    let has_exploration = journal.entries.iter().any(|e| matches!(e,
        JournalEntry::SequenceExplored { .. } | JournalEntry::BranchCreated { .. }
    ));

    if has_exploration {
        writeln!(md).unwrap();
        writeln!(md, "## Exploration Log\n").unwrap();
        for entry in &journal.entries {
            match entry {
                JournalEntry::SequenceExplored { steps, timestamp, .. } => {
                    writeln!(md, "- **{}** Explored: {}", &timestamp[..std::cmp::min(16, timestamp.len())], steps.join(" → ")).unwrap();
                }
                JournalEntry::BranchCreated { from_function, branch_function, timestamp } => {
                    writeln!(md, "- **{}** Branch: {} → {}", &timestamp[..std::cmp::min(16, timestamp.len())], from_function, branch_function).unwrap();
                }
                _ => {}
            }
        }
    }

    md
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_journal_export() {
        let j = AuditJournal::new("myproject", "Staking", "2026-03-31T10:00:00Z");
        let md = export_markdown(&j, 8);
        assert!(md.contains("# Audit: Staking"));
        assert!(md.contains("0/8 functions (0%)"));
        assert!(md.contains("No findings recorded."));
        assert!(md.contains("No functions reviewed yet."));
    }

    #[test]
    fn export_with_findings_and_status() {
        let mut j = AuditJournal::new("myproject", "Staking", "2026-03-31T10:00:00Z");

        j.add_finding(Finding {
            id: String::new(),
            severity: Severity::High,
            title: "Reentrancy in withdraw".into(),
            affected_function: "withdraw".into(),
            affected_sequence: Some(vec!["deposit".into(), "withdraw".into()]),
            description: "External call before state update".into(),
            notes: vec![],
            created_at: String::new(),
        }, "2026-03-31T14:30:00Z");

        j.record(JournalEntry::StatusChanged {
            function: "deposit".into(),
            status: ReviewStatus::Clean,
            timestamp: "2026-03-31T14:00:00Z".into(),
        });
        j.record(JournalEntry::StatusChanged {
            function: "withdraw".into(),
            status: ReviewStatus::Vulnerable,
            timestamp: "2026-03-31T14:30:00Z".into(),
        });

        let md = export_markdown(&j, 8);
        assert!(md.contains("2/8 functions (25%)"));
        assert!(md.contains("| F-01 | High | Reentrancy in withdraw | withdraw |"));
        assert!(md.contains("[ok] deposit"));
        assert!(md.contains("[!] withdraw"));
        assert!(md.contains("deposit → withdraw"));
    }

    #[test]
    fn export_with_exploration_log() {
        let mut j = AuditJournal::new("p", "c", "t");
        j.record(JournalEntry::SequenceExplored {
            steps: vec!["deposit".into(), "withdraw".into()],
            timestamp: "2026-03-31T14:32:00Z".into(),
        });
        j.record(JournalEntry::BranchCreated {
            from_function: "deposit".into(),
            branch_function: "claimRewards".into(),
            timestamp: "2026-03-31T14:45:00Z".into(),
        });

        let md = export_markdown(&j, 5);
        assert!(md.contains("## Exploration Log"));
        assert!(md.contains("Explored: deposit → withdraw"));
        assert!(md.contains("Branch: deposit → claimRewards"));
    }

    #[test]
    fn timestamp_truncation_handles_short_strings() {
        let mut j = AuditJournal::new("p", "c", "t");
        j.record(JournalEntry::SequenceExplored {
            steps: vec!["x".into()],
            timestamp: "short".into(),
        });
        let md = export_markdown(&j, 1);
        assert!(md.contains("**short**"));
    }
}
