// SPDX-License-Identifier: MIT OR Apache-2.0
//! The assurance gate. It reads the assurance graph (`docs/assurance/graph.json`),
//! checks that every piece of evidence it names exists, and sorts each claim
//! into one of three states:
//!
//! - **bounded-proven**: at least one `exhaustive` or `proof` item backs it. An
//!   exhaustive item is a finite-model check, so the claim holds within its
//!   stated bound.
//! - **tested**: only sampled evidence (`test`, `property`, `fuzz`) backs it.
//!   This is never reported as a proof.
//! - **unproven**: no evidence at all.
//!
//! The gate fails when a referenced item is missing, or when a critical claim is
//! unproven and has no waiver. A waiver names the reason the claim stays open, so
//! it shows in the report. A waiver on a claim that is now bounded-proven, or on a
//! claim that is not critical, also fails, so stale waivers are removed.

use std::fmt::Write as _;
use std::path::Path;

use serde_json::Value;

/// Evidence kinds the graph accepts.
pub const EVIDENCE_KINDS: [&str; 5] = ["test", "property", "fuzz", "exhaustive", "proof"];

/// Kinds that count toward a bounded proof.
const PROOF_GRADE: [&str; 2] = ["exhaustive", "proof"];

/// One item of evidence. `name` is a test function, a fuzz target or a proof
/// name, and `path` is the file that holds it, relative to the repository root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evidence {
    /// One of [`EVIDENCE_KINDS`].
    pub kind: String,
    /// The file, relative to the repository root.
    pub path: String,
    /// The function, target or proof name.
    pub name: String,
}

/// A claim the project makes, with the evidence that backs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    /// A stable identifier such as `CAP-1`.
    pub id: String,
    /// The layer or crate the claim is about.
    pub layer: String,
    /// True if a release depends on this claim.
    pub critical: bool,
    /// The claim, in one sentence.
    pub statement: String,
    /// The evidence that backs it.
    pub evidence: Vec<Evidence>,
    /// Why a critical claim stays open. Empty or absent when there is none.
    pub waiver: Option<String>,
}

/// Where a claim stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Backed by an exhaustive or proof item.
    BoundedProven,
    /// Backed only by sampled evidence.
    Tested,
    /// Backed by no evidence, and waived.
    Waived,
    /// Backed by no evidence.
    Unproven,
}

impl Status {
    /// The label used in the report.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::BoundedProven => "bounded-proven",
            Self::Tested => "tested",
            Self::Waived => "waived",
            Self::Unproven => "UNPROVEN",
        }
    }
}

/// The result for one claim.
#[derive(Debug)]
pub struct Outcome {
    /// The claim.
    pub claim: Claim,
    /// Its status.
    pub status: Status,
    /// Problems with this claim. Any problem fails the gate.
    pub problems: Vec<String>,
}

/// The whole graph, checked.
#[derive(Debug)]
pub struct Report {
    /// One outcome per claim, in graph order.
    pub outcomes: Vec<Outcome>,
}

/// Parses the graph. Every claim needs an `id`, `layer`, `statement` and an
/// `evidence` array. Each evidence item needs a known `kind`, a `path` and a
/// `name`. Claim IDs must be unique.
///
/// # Errors
///
/// Returns a message naming the first thing that is malformed.
pub fn parse(text: &str) -> Result<Vec<Claim>, String> {
    let root: Value = serde_json::from_str(text).map_err(|e| format!("graph is not JSON: {e}"))?;
    let claims = root
        .get("claims")
        .and_then(Value::as_array)
        .ok_or("graph has no `claims` array")?;
    let mut parsed: Vec<Claim> = Vec::new();
    for entry in claims {
        let claim = parse_claim(entry)?;
        if parsed.iter().any(|c| c.id == claim.id) {
            return Err(format!("claim `{}` is defined twice", claim.id));
        }
        parsed.push(claim);
    }
    Ok(parsed)
}

fn string_field<'a>(value: &'a Value, field: &str, owner: &str) -> Result<&'a str, String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{owner}: `{field}` must be a string"))
}

fn parse_claim(entry: &Value) -> Result<Claim, String> {
    let id = string_field(entry, "id", "claim")?.to_string();
    let owner = format!("claim `{id}`");
    let layer = string_field(entry, "layer", &owner)?.to_string();
    let statement = string_field(entry, "statement", &owner)?.to_string();
    let critical = match entry.get("critical") {
        None => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(format!("{owner}: `critical` must be true or false")),
    };
    let waiver = match entry.get("waiver") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if !s.trim().is_empty() => Some(s.clone()),
        Some(_) => return Err(format!("{owner}: `waiver` must be a non-empty string")),
    };
    let items = entry
        .get("evidence")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{owner}: `evidence` must be an array"))?;
    let mut evidence = Vec::new();
    for item in items {
        let kind = string_field(item, "kind", &owner)?;
        if !EVIDENCE_KINDS.contains(&kind) {
            return Err(format!("{owner}: unknown evidence kind `{kind}`"));
        }
        evidence.push(Evidence {
            kind: kind.to_string(),
            path: string_field(item, "path", &owner)?.to_string(),
            name: string_field(item, "name", &owner)?.to_string(),
        });
    }
    Ok(Claim {
        id,
        layer,
        critical,
        statement,
        evidence,
        waiver,
    })
}

/// Checks every claim against the files under `root`.
#[must_use]
pub fn evaluate(claims: &[Claim], root: &Path) -> Report {
    let outcomes = claims
        .iter()
        .map(|claim| evaluate_claim(claim, root))
        .collect();
    Report { outcomes }
}

fn evaluate_claim(claim: &Claim, root: &Path) -> Outcome {
    let mut problems = Vec::new();
    for ev in &claim.evidence {
        let file = root.join(&ev.path);
        match std::fs::read_to_string(&file) {
            Err(_) => problems.push(format!("evidence file `{}` does not exist", ev.path)),
            Ok(source) => {
                // Test-like items are Rust functions, so the name must appear as
                // a function definition in the file.
                if matches!(ev.kind.as_str(), "test" | "property" | "exhaustive")
                    && !source.contains(&format!("fn {}(", ev.name))
                {
                    problems.push(format!("`fn {}(` not found in `{}`", ev.name, ev.path));
                }
            }
        }
    }

    let proven = claim
        .evidence
        .iter()
        .any(|ev| PROOF_GRADE.contains(&ev.kind.as_str()));
    let status = if proven {
        Status::BoundedProven
    } else if !claim.evidence.is_empty() {
        Status::Tested
    } else if claim.waiver.is_some() {
        Status::Waived
    } else {
        Status::Unproven
    };

    match (&claim.waiver, status) {
        (None, s) if claim.critical && s != Status::BoundedProven => {
            problems.push("critical claim is not bounded-proven and has no waiver".to_string())
        }
        (Some(_), Status::BoundedProven) => {
            problems.push("waiver is stale: the claim is bounded-proven".to_string());
        }
        (Some(_), _) if !claim.critical => {
            problems.push("waiver on a claim that is not critical".to_string());
        }
        _ => {}
    }

    Outcome {
        claim: claim.clone(),
        status,
        problems,
    }
}

impl Report {
    /// Every problem, prefixed with its claim ID.
    #[must_use]
    pub fn problems(&self) -> Vec<String> {
        self.outcomes
            .iter()
            .flat_map(|o| {
                o.problems
                    .iter()
                    .map(move |p| format!("{}: {p}", o.claim.id))
            })
            .collect()
    }

    /// True if no claim has a problem.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.outcomes.iter().all(|o| o.problems.is_empty())
    }

    /// The report as Markdown: a summary, one row per claim, and the waivers.
    #[must_use]
    pub fn render(&self) -> String {
        let count = |s: Status| self.outcomes.iter().filter(|o| o.status == s).count();
        let mut out = String::new();
        out.push_str("# Assurance report\n\n");
        out.push_str(
            "Generated by `tpt-k-build assure` from `docs/assurance/graph.json`. \
             Do not edit by hand.\n\n",
        );
        out.push_str(
            "- **bounded-proven**: backed by an exhaustive check over a finite model, \
             which holds within the stated bound.\n\
             - **tested**: backed only by sampled evidence (tests, property tests, fuzzing). \
             This is not a proof.\n\
             - **waived**: the claim is not bounded-proven and has a waiver that says why it \
             stays open. Waivers are listed below the table.\n\
             - **UNPROVEN**: no evidence and no waiver.\n\n",
        );
        let waivers = self
            .outcomes
            .iter()
            .filter(|o| o.claim.waiver.is_some() && o.status != Status::BoundedProven)
            .count();
        let _ = writeln!(
            out,
            "Claims: {} total, {} bounded-proven, {} tested, {} with a waiver, {} unproven.\n",
            self.outcomes.len(),
            count(Status::BoundedProven),
            count(Status::Tested),
            waivers,
            count(Status::Unproven),
        );
        out.push_str("| Claim | Layer | Critical | Status | Evidence |\n");
        out.push_str("|---|---|---|---|---|\n");
        for o in &self.outcomes {
            let evidence = if o.claim.evidence.is_empty() {
                "none".to_string()
            } else {
                o.claim
                    .evidence
                    .iter()
                    .map(|e| format!("{} `{}`", e.kind, e.name))
                    .collect::<Vec<_>>()
                    .join("; ")
            };
            let _ = writeln!(
                out,
                "| {} {} | {} | {} | {} | {} |",
                o.claim.id,
                o.claim.statement.replace('|', "\\|"),
                o.claim.layer,
                if o.claim.critical { "yes" } else { "no" },
                o.status.label(),
                evidence.replace('|', "\\|"),
            );
        }
        let waived: Vec<&Outcome> = self
            .outcomes
            .iter()
            .filter(|o| o.claim.waiver.is_some() && o.status != Status::BoundedProven)
            .collect();
        if !waived.is_empty() {
            out.push_str("\n## Waivers\n\n");
            for o in waived {
                let reason = o.claim.waiver.as_deref().unwrap_or("");
                let _ = writeln!(out, "- **{}**: {reason}", o.claim.id);
            }
        }
        let problems = self.problems();
        if !problems.is_empty() {
            out.push_str("\n## Problems\n\n");
            for p in problems {
                let _ = writeln!(out, "- {p}");
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A scratch directory with one source file, unique per test.
    fn scratch(name: &str, source: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tpt-k-assure-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("lib.rs"), source).unwrap();
        dir
    }

    fn claim(id: &str, critical: bool, evidence: &[(&str, &str)], waiver: Option<&str>) -> Claim {
        Claim {
            id: id.to_string(),
            layer: "test".to_string(),
            critical,
            statement: "a statement".to_string(),
            evidence: evidence
                .iter()
                .map(|(kind, name)| Evidence {
                    kind: kind.to_string(),
                    path: "lib.rs".to_string(),
                    name: name.to_string(),
                })
                .collect(),
            waiver: waiver.map(str::to_string),
        }
    }

    #[test]
    fn exhaustive_evidence_makes_a_claim_bounded_proven() {
        let root = scratch("proven", "fn check_all() {}\nfn sample() {}\n");
        let r = evaluate(
            &[claim(
                "A",
                true,
                &[("exhaustive", "check_all"), ("test", "sample")],
                None,
            )],
            &root,
        );
        assert!(r.passed(), "{:?}", r.problems());
        assert_eq!(r.outcomes[0].status, Status::BoundedProven);
    }

    #[test]
    fn sampled_evidence_is_tested_and_not_a_proof() {
        let root = scratch("tested", "fn sample() {}\n");
        let r = evaluate(&[claim("B", false, &[("test", "sample")], None)], &root);
        assert_eq!(r.outcomes[0].status, Status::Tested);
        assert!(r.passed());
    }

    #[test]
    fn critical_unproven_claim_fails_the_gate() {
        let root = scratch("unproven", "");
        let r = evaluate(&[claim("C", true, &[], None)], &root);
        assert!(!r.passed());
        assert_eq!(r.outcomes[0].status, Status::Unproven);
    }

    #[test]
    fn critical_claim_that_is_only_tested_needs_a_waiver() {
        // Sampled evidence is not a proof, so a critical claim backed only by
        // tests fails the gate unless it is waived.
        let root = scratch(
            "tested-critical",
            "fn sample() {}
",
        );
        let r = evaluate(&[claim("H", true, &[("test", "sample")], None)], &root);
        assert!(!r.passed());
        let waived = evaluate(
            &[claim(
                "H",
                true,
                &[("test", "sample")],
                Some("no model yet"),
            )],
            &root,
        );
        assert!(waived.passed(), "{:?}", waived.problems());
        assert_eq!(waived.outcomes[0].status, Status::Tested);
        assert!(waived.render().contains("no model yet"));
    }

    #[test]
    fn waiver_keeps_a_critical_unproven_claim_visible_and_passing() {
        let root = scratch("waived", "");
        let r = evaluate(&[claim("D", true, &[], Some("formal model open"))], &root);
        assert!(r.passed(), "{:?}", r.problems());
        assert_eq!(r.outcomes[0].status, Status::Waived);
        assert!(r.render().contains("formal model open"));
    }

    #[test]
    fn stale_waiver_on_a_proven_claim_fails() {
        let root = scratch("stale", "fn check_all() {}\n");
        let r = evaluate(
            &[claim(
                "E",
                true,
                &[("exhaustive", "check_all")],
                Some("old"),
            )],
            &root,
        );
        assert!(!r.passed());
    }

    #[test]
    fn missing_function_fails_the_gate() {
        let root = scratch("missing", "fn other() {}\n");
        let r = evaluate(&[claim("F", false, &[("test", "absent")], None)], &root);
        assert!(!r.passed());
        assert!(r.problems()[0].contains("fn absent("));
    }

    #[test]
    fn missing_file_fails_the_gate() {
        let root = scratch("nofile", "");
        let mut c = claim("G", false, &[("fuzz", "target")], None);
        c.evidence[0].path = "no/such/file.rs".to_string();
        let r = evaluate(&[c], &root);
        assert!(!r.passed());
    }

    #[test]
    fn parse_rejects_unknown_kind_and_duplicate_ids() {
        let bad_kind = r#"{"claims":[{"id":"X","layer":"l","statement":"s",
            "evidence":[{"kind":"guess","path":"p","name":"n"}]}]}"#;
        assert!(parse(bad_kind)
            .unwrap_err()
            .contains("unknown evidence kind"));
        let dup = r#"{"claims":[
            {"id":"X","layer":"l","statement":"s","evidence":[]},
            {"id":"X","layer":"l","statement":"s","evidence":[]}]}"#;
        assert!(parse(dup).unwrap_err().contains("defined twice"));
    }

    #[test]
    fn parse_reads_a_well_formed_graph() {
        let text = r#"{"claims":[{"id":"X","layer":"l","critical":true,"statement":"s",
            "evidence":[{"kind":"test","path":"p.rs","name":"n"}],"waiver":null}]}"#;
        let claims = parse(text).unwrap();
        assert_eq!(claims.len(), 1);
        assert!(claims[0].critical);
        assert_eq!(claims[0].evidence[0].name, "n");
        assert_eq!(claims[0].waiver, None);
    }

    #[test]
    fn repository_graph_passes_the_gate() {
        // The real graph must pass, so `cargo test` enforces the gate as well as
        // the script does.
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
        let text = std::fs::read_to_string(repo.join("docs/assurance/graph.json")).unwrap();
        let claims = parse(&text).unwrap();
        let report = evaluate(&claims, &repo);
        assert!(report.passed(), "{:#?}", report.problems());
    }
}
