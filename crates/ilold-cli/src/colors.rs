use colored::{Colorize, ColoredString};
use ilold_core::classify::entry_points::AccessLevel;
use ilold_core::classify::guards::{GuardFact, GuardScope};

pub fn c_accent(s: &str) -> ColoredString { s.truecolor(140, 170, 210) }
pub fn c_warn(s: &str) -> ColoredString { s.truecolor(180, 150, 80) }
pub fn c_danger(s: &str) -> ColoredString { s.truecolor(170, 90, 90) }
pub fn c_ok(s: &str) -> ColoredString { s.truecolor(100, 160, 110) }
pub fn c_muted(s: &str) -> ColoredString { s.truecolor(110, 120, 140) }
pub fn c_bright(s: &str) -> ColoredString { s.truecolor(190, 200, 215).bold() }

pub fn access_colored(access: &AccessLevel) -> ColoredString {
    match access {
        AccessLevel::Public => c_accent("[P]"),
        AccessLevel::Restricted { .. } => c_warn("[R]"),
        AccessLevel::Internal => c_muted("[I]"),
        AccessLevel::Special { .. } => c_muted("[S]"),
    }
}

/// `?` marks guards taken from a modifier name, `*` checks that hold on some paths only.
pub fn guard_badges(guards: &[GuardFact]) -> String {
    let mut tags: Vec<String> = Vec::new();
    for g in guards {
        let conditional = if g.scope == GuardScope::Conditional { "*" } else { "" };
        let tag = format!("{{{}{}{conditional}}}", g.kind.label(), if g.heuristic { "?" } else { "" });
        if !tags.contains(&tag) {
            tags.push(tag);
        }
    }
    tags.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilold_core::classify::guards::GuardKind;

    fn guard(kind: GuardKind, heuristic: bool) -> GuardFact {
        GuardFact {
            kind,
            condition: String::new(),
            via: Vec::new(),
            span: None,
            heuristic,
            scope: GuardScope::Unconditional,
            compared_against: None,
            checked_by: None,
        }
    }

    #[test]
    fn guard_badges_one_per_kind_marking_name_based_ones() {
        let guards = [
            guard(GuardKind::CallerEquals, false),
            guard(GuardKind::CallerEquals, false),
            guard(GuardKind::Pause, false),
            guard(GuardKind::AccessByName, true),
            GuardFact { scope: GuardScope::Conditional, ..guard(GuardKind::RoleCheck, false) },
        ];
        assert_eq!(guard_badges(&guards), "{caller} {pause} {access?} {role*}");
        assert_eq!(guard_badges(&[]), "");
    }
}
