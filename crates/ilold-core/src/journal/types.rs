use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditJournal {
    pub version: u32,
    pub project: String,
    pub contract: String,
    pub started_at: String,
    pub entries: Vec<JournalEntry>,
    pub findings: Vec<Finding>,
    pub function_status: HashMap<String, ReviewStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JournalEntry {
    FunctionAdded { name: String, timestamp: String },
    SequenceExplored { steps: Vec<String>, timestamp: String },
    BranchCreated { from_function: String, branch_function: String, timestamp: String },
    FindingRecorded { finding_id: String, timestamp: String },
    NoteAdded { anchor: String, content: String, timestamp: String },
    StatusChanged { function: String, status: ReviewStatus, timestamp: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub id: String,
    pub severity: Severity,
    pub title: String,
    pub affected_function: String,
    pub affected_sequence: Option<Vec<String>>,
    pub description: String,
    pub notes: Vec<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
    Informational,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Severity::Critical => write!(f, "Critical"),
            Severity::High => write!(f, "High"),
            Severity::Medium => write!(f, "Medium"),
            Severity::Low => write!(f, "Low"),
            Severity::Informational => write!(f, "Info"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewStatus {
    NotReviewed,
    InProgress,
    Reviewed,
    Suspicious,
    Vulnerable,
    Clean,
}

impl fmt::Display for ReviewStatus {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ReviewStatus::NotReviewed => write!(f, "Not reviewed"),
            ReviewStatus::InProgress => write!(f, "In progress"),
            ReviewStatus::Reviewed => write!(f, "Reviewed"),
            ReviewStatus::Suspicious => write!(f, "Suspicious"),
            ReviewStatus::Vulnerable => write!(f, "Vulnerable"),
            ReviewStatus::Clean => write!(f, "Clean"),
        }
    }
}

impl ReviewStatus {
    pub fn badge(&self) -> &str {
        match self {
            ReviewStatus::NotReviewed => "[ ]",
            ReviewStatus::InProgress => "[~]",
            ReviewStatus::Reviewed => "[R]",
            ReviewStatus::Suspicious => "[?]",
            ReviewStatus::Vulnerable => "[!]",
            ReviewStatus::Clean => "[ok]",
        }
    }

    pub fn is_done(&self) -> bool {
        matches!(self, ReviewStatus::Reviewed | ReviewStatus::Clean | ReviewStatus::Vulnerable)
    }
}

impl AuditJournal {
    pub fn new(project: &str, contract: &str, started_at: &str) -> Self {
        AuditJournal {
            version: 1,
            project: project.into(),
            contract: contract.into(),
            started_at: started_at.into(),
            entries: Vec::new(),
            findings: Vec::new(),
            function_status: HashMap::new(),
        }
    }

    pub fn record(&mut self, entry: JournalEntry) {
        if let JournalEntry::StatusChanged { ref function, ref status, .. } = entry {
            self.function_status.insert(function.clone(), *status);
        }
        self.entries.push(entry);
    }

    pub fn add_finding(&mut self, mut finding: Finding, timestamp: &str) {
        let id = format!("F-{:02}", self.findings.len() + 1);
        finding.id = id.clone();
        finding.created_at = timestamp.into();
        self.findings.push(finding);
        self.record(JournalEntry::FindingRecorded {
            finding_id: id,
            timestamp: timestamp.into(),
        });
    }

    pub fn progress(&self, total_functions: usize) -> (usize, usize) {
        let done = self.function_status.values().filter(|s| s.is_done()).count();
        (done, total_functions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_journal_is_empty() {
        let j = AuditJournal::new("myproject", "Staking", "2026-03-31T10:00:00Z");
        assert_eq!(j.entries.len(), 0);
        assert_eq!(j.findings.len(), 0);
        assert_eq!(j.version, 1);
    }

    #[test]
    fn record_updates_status() {
        let mut j = AuditJournal::new("p", "c", "t");
        j.record(JournalEntry::StatusChanged {
            function: "deposit".into(),
            status: ReviewStatus::Reviewed,
            timestamp: "t".into(),
        });
        assert_eq!(j.function_status.get("deposit"), Some(&ReviewStatus::Reviewed));
        assert_eq!(j.entries.len(), 1);
    }

    #[test]
    fn add_finding_auto_id() {
        let mut j = AuditJournal::new("p", "c", "t");
        j.add_finding(Finding {
            id: String::new(),
            severity: Severity::High,
            title: "Reentrancy".into(),
            affected_function: "withdraw".into(),
            affected_sequence: Some(vec!["deposit".into(), "withdraw".into()]),
            description: "CEI violation".into(),
            notes: vec![],
            created_at: String::new(),
        }, "2026-03-31T10:00:00Z");

        assert_eq!(j.findings.len(), 1);
        assert_eq!(j.findings[0].id, "F-01");
        assert_eq!(j.findings[0].created_at, "2026-03-31T10:00:00Z");
        assert_eq!(j.entries.len(), 1);
    }

    #[test]
    fn progress_counts_done() {
        let mut j = AuditJournal::new("p", "c", "t");
        j.record(JournalEntry::StatusChanged {
            function: "deposit".into(), status: ReviewStatus::Clean, timestamp: "t".into(),
        });
        j.record(JournalEntry::StatusChanged {
            function: "withdraw".into(), status: ReviewStatus::Vulnerable, timestamp: "t".into(),
        });
        j.record(JournalEntry::StatusChanged {
            function: "claim".into(), status: ReviewStatus::InProgress, timestamp: "t".into(),
        });
        let (done, total) = j.progress(8);
        assert_eq!(done, 2);
        assert_eq!(total, 8);
    }

    #[test]
    fn status_overwrite_keeps_latest() {
        let mut j = AuditJournal::new("p", "c", "t");
        j.record(JournalEntry::StatusChanged {
            function: "deposit".into(), status: ReviewStatus::InProgress, timestamp: "t1".into(),
        });
        j.record(JournalEntry::StatusChanged {
            function: "deposit".into(), status: ReviewStatus::Clean, timestamp: "t2".into(),
        });
        assert_eq!(j.function_status.get("deposit"), Some(&ReviewStatus::Clean));
        assert_eq!(j.entries.len(), 2);
    }

    #[test]
    fn multiple_findings_sequential_ids() {
        let mut j = AuditJournal::new("p", "c", "t");
        let f = Finding {
            id: String::new(), severity: Severity::High, title: "A".into(),
            affected_function: "x".into(), affected_sequence: None,
            description: "d".into(), notes: vec![], created_at: String::new(),
        };
        j.add_finding(f.clone(), "t1");
        j.add_finding(f.clone(), "t2");
        j.add_finding(f, "t3");
        assert_eq!(j.findings[0].id, "F-01");
        assert_eq!(j.findings[1].id, "F-02");
        assert_eq!(j.findings[2].id, "F-03");
    }

    #[test]
    fn status_badge() {
        assert_eq!(ReviewStatus::Clean.badge(), "[ok]");
        assert_eq!(ReviewStatus::Vulnerable.badge(), "[!]");
        assert_eq!(ReviewStatus::NotReviewed.badge(), "[ ]");
    }
}
