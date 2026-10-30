use std::fmt::Write;

use super::types::*;

const SESSION_LEVEL: &str = "(session-level)";

pub fn export_markdown(journal: &AuditJournal, total_functions: usize) -> String {
    let mut md = String::new();
    let (done, total) = journal.progress(total_functions);
    let pct = if total > 0 { done * 100 / total } else { 0 };

    writeln!(md, "# Audit: {}", journal.contract).unwrap();
    writeln!(md, "**Project**: {} | **Started**: {} | **Progress**: {}/{} functions ({}%)",
        journal.project, format_timestamp(started_at(journal)), done, total, pct).unwrap();

    writeln!(md).unwrap();
    if journal.findings.is_empty() {
        writeln!(md, "## Findings\n\nNo findings recorded.").unwrap();
    } else {
        writeln!(md, "## Findings\n").unwrap();
        writeln!(md, "| ID | Severity | Title | Function |").unwrap();
        writeln!(md, "|----|----------|-------|----------|").unwrap();
        for f in &journal.findings {
            writeln!(md, "| {} | {} | {} | {} |", f.id, f.severity, f.title, function_label(f)).unwrap();
        }

        for f in &journal.findings {
            writeln!(md).unwrap();
            writeln!(md, "### {}: {}\n", f.id, f.title).unwrap();
            writeln!(md, "**Severity**: {} | **Function**: {} | **Recorded**: {}",
                f.severity, function_label(f), format_timestamp(&f.created_at)).unwrap();
            if let Some(seq) = &f.affected_sequence {
                writeln!(md, "**Sequence**: {}", seq.join(" → ")).unwrap();
            }
            writeln!(md, "\n{}", f.description).unwrap();
            for note in &f.notes {
                writeln!(md, "\n> {}", note).unwrap();
            }
        }
    }

    let notes: Vec<(&str, &str, &str)> = journal.entries.iter().filter_map(|e| match e {
        JournalEntry::NoteAdded { anchor, content, timestamp } => {
            Some((anchor.as_str(), content.as_str(), timestamp.as_str()))
        }
        _ => None,
    }).collect();
    if !notes.is_empty() {
        writeln!(md).unwrap();
        writeln!(md, "## Notes\n").unwrap();
        for (anchor, content, timestamp) in notes {
            let anchor = if anchor.is_empty() { SESSION_LEVEL.to_string() } else { format!("(after {anchor})") };
            writeln!(md, "- **{}** {} {}", format_timestamp(timestamp), anchor, content).unwrap();
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
                    writeln!(md, "- **{}** Explored: {}", format_timestamp(timestamp), steps.join(" → ")).unwrap();
                }
                JournalEntry::BranchCreated { from_function, branch_function, timestamp } => {
                    writeln!(md, "- **{}** Branch: {} → {}", format_timestamp(timestamp), from_function, branch_function).unwrap();
                }
                _ => {}
            }
        }
    }

    md
}

fn function_label(f: &Finding) -> &str {
    if f.affected_function.is_empty() { SESSION_LEVEL } else { &f.affected_function }
}

/// Journal start, or the first recorded entry for journals saved without one.
fn started_at(journal: &AuditJournal) -> &str {
    if !journal.started_at.is_empty() {
        return &journal.started_at;
    }
    journal.entries.iter().map(entry_timestamp).find(|t| !t.is_empty()).unwrap_or("")
}

fn entry_timestamp(entry: &JournalEntry) -> &str {
    match entry {
        JournalEntry::FunctionAdded { timestamp, .. }
        | JournalEntry::SequenceExplored { timestamp, .. }
        | JournalEntry::BranchCreated { timestamp, .. }
        | JournalEntry::FindingRecorded { timestamp, .. }
        | JournalEntry::NoteAdded { timestamp, .. }
        | JournalEntry::StatusChanged { timestamp, .. } => timestamp,
    }
}

/// Unix-seconds timestamps (what the server records) as `YYYY-MM-DD HH:MM:SS UTC`;
/// other strings keep their first 16 characters (ISO minutes).
pub fn format_timestamp(ts: &str) -> String {
    if ts.is_empty() {
        return "unknown".into();
    }
    match ts.parse::<u64>() {
        Ok(secs) => {
            let (y, m, d) = civil_from_days((secs / 86_400) as i64);
            let rem = secs % 86_400;
            format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC", rem / 3600, rem % 3600 / 60, rem % 60)
        }
        Err(_) => ts.chars().take(16).collect(),
    }
}

/// Days since 1970-01-01 to a proleptic Gregorian date (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y, m, d)
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
    fn header_has_readable_start_and_epoch_timestamps_are_formatted() {
        let mut j = AuditJournal::new("aave-v3-origin", "Pool", "1793355240");
        j.record(JournalEntry::SequenceExplored {
            steps: vec!["supply".into()],
            timestamp: "1793355300".into(),
        });
        let md = export_markdown(&j, 1);
        assert!(md.contains("**Project**: aave-v3-origin | **Started**: 2026-10-30 10:14:00 UTC"), "{md}");
        assert!(md.contains("**2026-10-30 10:15:00 UTC** Explored: supply"), "{md}");
        assert!(!md.contains("1793355300"), "{md}");
    }

    #[test]
    fn empty_start_falls_back_to_the_first_entry() {
        let mut j = AuditJournal::new("p", "c", "");
        j.record(JournalEntry::NoteAdded {
            anchor: String::new(),
            content: "look at fees".into(),
            timestamp: "1793355240".into(),
        });
        let md = export_markdown(&j, 1);
        assert!(md.contains("**Started**: 2026-10-30 10:14:00 UTC"), "{md}");
    }

    #[test]
    fn notes_are_exported_with_their_anchor() {
        let mut j = AuditJournal::new("p", "c", "1793355240");
        j.record(JournalEntry::NoteAdded {
            anchor: String::new(),
            content: "session-wide remark".into(),
            timestamp: "1793355240".into(),
        });
        j.record(JournalEntry::NoteAdded {
            anchor: "deposit → withdraw".into(),
            content: "withdraw reads stale balance".into(),
            timestamp: "1793355300".into(),
        });
        let md = export_markdown(&j, 1);
        assert!(md.contains("## Notes"), "{md}");
        assert!(md.contains("(session-level) session-wide remark"), "{md}");
        assert!(md.contains("(after deposit → withdraw) withdraw reads stale balance"), "{md}");
    }

    #[test]
    fn session_level_finding_is_labelled() {
        let mut j = AuditJournal::new("p", "c", "t");
        j.add_finding(Finding {
            id: String::new(),
            severity: Severity::Low,
            title: "Missing events".into(),
            affected_function: String::new(),
            affected_sequence: None,
            description: String::new(),
            notes: vec![],
            created_at: String::new(),
        }, "1793355240");
        let md = export_markdown(&j, 1);
        assert!(md.contains("| F-01 | Low | Missing events | (session-level) |"), "{md}");
    }

    #[test]
    fn civil_date_conversion() {
        assert_eq!(format_timestamp("0"), "1970-01-01 00:00:00 UTC");
        assert_eq!(format_timestamp("951782400"), "2000-02-29 00:00:00 UTC");
        assert_eq!(format_timestamp(""), "unknown");
        assert_eq!(format_timestamp("2026-03-31T14:32:00Z"), "2026-03-31T14:32");
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
