// SPDX-License-Identifier: MIT OR Apache-2.0
//! Policy as a manifest. An application manifest names the objects it may use and
//! the rights each process gets on them. [`compile`] turns the manifest into the
//! grants the root server makes, checking every grant against the object's declared
//! ceiling. [`lint`] flags grants that are broader than the process needs.
//!
//! The manifest is TOML:
//!
//! ```toml
//! [objects.console]
//! rights = ["read", "write"]          # the most any process may hold
//!
//! [objects.config]
//! rights = ["read", "write", "grant"]
//!
//! [[process]]
//! name = "shell"
//! grants = [ { object = "console", rights = ["read", "write"] } ]
//!
//! [[process]]
//! name = "loader"
//! grants = [ { object = "config", rights = ["read", "grant"] } ]
//! ```

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::Deserialize;
use tpt_k_kernel_ipc::cap::Rights;

/// The rights names a manifest may use, with the rights they stand for.
const NAMES: [(&str, Rights); 3] = [
    ("read", Rights::READ),
    ("write", Rights::WRITE),
    ("grant", Rights::GRANT),
];

#[derive(Debug, Deserialize)]
struct RawManifest {
    #[serde(default)]
    objects: BTreeMap<String, RawObject>,
    #[serde(default)]
    process: Vec<RawProcess>,
}

#[derive(Debug, Deserialize)]
struct RawObject {
    rights: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RawProcess {
    name: String,
    #[serde(default)]
    grants: Vec<RawGrant>,
}

#[derive(Debug, Deserialize)]
struct RawGrant {
    object: String,
    rights: Vec<String>,
}

/// A parsed manifest, before its grants are checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// Each declared object and the most rights any process may hold on it.
    pub objects: BTreeMap<String, Rights>,
    /// The processes, in manifest order.
    pub processes: Vec<ProcessSpec>,
}

/// One process and the grants it asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessSpec {
    pub name: String,
    pub grants: Vec<GrantSpec>,
}

/// One requested grant: an object and the rights asked for on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantSpec {
    pub object: String,
    pub rights: Rights,
}

/// A grant the root server makes: the process, the object, and the rights.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grant<'a> {
    pub process: &'a str,
    pub object: &'a str,
    pub rights: Rights,
}

/// Why a manifest is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestError {
    /// The TOML does not parse, or does not have the expected shape.
    Parse(String),
    /// A rights name is not `read`, `write` or `grant`.
    UnknownRight { object: String, right: String },
    /// A grant names an object that the manifest does not declare.
    UnknownObject { process: String, object: String },
    /// A grant asks for a right above the object's declared ceiling.
    AboveCeiling {
        process: String,
        object: String,
        right: String,
    },
    /// Two processes have the same name.
    DuplicateProcess(String),
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(e) => write!(f, "manifest: {e}"),
            Self::UnknownRight { object, right } => {
                write!(f, "object `{object}`: unknown right `{right}` (use read, write or grant)")
            }
            Self::UnknownObject { process, object } => {
                write!(f, "process `{process}` is granted `{object}`, which is not declared")
            }
            Self::AboveCeiling {
                process,
                object,
                right,
            } => write!(
                f,
                "process `{process}` asks for `{right}` on `{object}`, above the object's declared rights"
            ),
            Self::DuplicateProcess(name) => write!(f, "process `{name}` is declared twice"),
        }
    }
}

impl std::error::Error for ManifestError {}

/// Turns a list of rights names into a mask. Each name must be known.
fn rights_from_names(object: &str, names: &[String]) -> Result<Rights, ManifestError> {
    let mut mask = Rights::NONE;
    for name in names {
        let (_, bit) = NAMES
            .iter()
            .find(|(known, _)| known == name)
            .ok_or_else(|| ManifestError::UnknownRight {
                object: object.to_string(),
                right: name.clone(),
            })?;
        mask = mask.union(*bit);
    }
    Ok(mask)
}

/// The names of the rights in `mask`, in the order `read`, `write`, `grant`.
#[must_use]
pub fn right_names(mask: Rights) -> Vec<&'static str> {
    NAMES
        .iter()
        .filter(|(_, bit)| mask.contains(*bit))
        .map(|(name, _)| *name)
        .collect()
}

/// Parses a manifest. It checks the shape and the rights names, not the grants.
///
/// # Errors
///
/// Returns [`ManifestError::Parse`] for bad TOML, [`ManifestError::UnknownRight`]
/// for a rights name that is not known, and [`ManifestError::DuplicateProcess`] for
/// a repeated process name.
pub fn parse(text: &str) -> Result<Manifest, ManifestError> {
    let raw: RawManifest = toml::from_str(text).map_err(|e| ManifestError::Parse(e.to_string()))?;
    let mut objects = BTreeMap::new();
    for (name, object) in &raw.objects {
        objects.insert(name.clone(), rights_from_names(name, &object.rights)?);
    }
    let mut seen = BTreeSet::new();
    let mut processes = Vec::new();
    for process in raw.process {
        if !seen.insert(process.name.clone()) {
            return Err(ManifestError::DuplicateProcess(process.name));
        }
        let mut grants = Vec::new();
        for grant in process.grants {
            grants.push(GrantSpec {
                rights: rights_from_names(&grant.object, &grant.rights)?,
                object: grant.object,
            });
        }
        processes.push(ProcessSpec {
            name: process.name,
            grants,
        });
    }
    Ok(Manifest { objects, processes })
}

/// Checks every requested grant against its object's ceiling, and returns the grants
/// to make, in manifest order.
///
/// # Errors
///
/// Returns [`ManifestError::UnknownObject`] for a grant on an undeclared object, and
/// [`ManifestError::AboveCeiling`] for a grant with a right the object does not allow.
pub fn compile(manifest: &Manifest) -> Result<Vec<Grant<'_>>, ManifestError> {
    let mut grants = Vec::new();
    for process in &manifest.processes {
        for grant in &process.grants {
            let ceiling = manifest.objects.get(&grant.object).ok_or_else(|| {
                ManifestError::UnknownObject {
                    process: process.name.clone(),
                    object: grant.object.clone(),
                }
            })?;
            if !ceiling.contains(grant.rights) {
                let right = NAMES
                    .iter()
                    .find(|(_, bit)| grant.rights.contains(*bit) && !ceiling.contains(*bit))
                    .map_or("?", |(name, _)| *name);
                return Err(ManifestError::AboveCeiling {
                    process: process.name.clone(),
                    object: grant.object.clone(),
                    right: right.to_string(),
                });
            }
            grants.push(Grant {
                process: &process.name,
                object: &grant.object,
                rights: grant.rights,
            });
        }
    }
    Ok(grants)
}

/// A place where a grant is broader than it needs to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// The process, or empty for a finding about an object.
    pub process: String,
    pub object: String,
    pub message: String,
}

/// Lints a manifest. It flags a grant that includes `grant`, because the holder can
/// then hand the object to other processes, and it flags a declared object that no
/// process is granted, since its ceiling is never used.
#[must_use]
pub fn lint(manifest: &Manifest) -> Vec<Finding> {
    let mut findings = Vec::new();
    for process in &manifest.processes {
        for grant in &process.grants {
            if grant.rights.contains(Rights::GRANT) {
                findings.push(Finding {
                    process: process.name.clone(),
                    object: grant.object.clone(),
                    message: "grants the `grant` right: the process can hand this object to \
                              others; keep it only where delegation is the point"
                        .to_string(),
                });
            }
        }
    }
    let used: BTreeSet<&str> = manifest
        .processes
        .iter()
        .flat_map(|p| p.grants.iter().map(|g| g.object.as_str()))
        .collect();
    for name in manifest.objects.keys() {
        if !used.contains(name.as_str()) {
            findings.push(Finding {
                process: String::new(),
                object: name.clone(),
                message: "declared but granted to no process".to_string(),
            });
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
[objects.console]
rights = ["read", "write"]

[objects.config]
rights = ["read", "write", "grant"]

[[process]]
name = "shell"
grants = [ { object = "console", rights = ["read", "write"] } ]

[[process]]
name = "loader"
grants = [ { object = "config", rights = ["read", "grant"] } ]
"#;

    #[test]
    fn a_good_manifest_compiles_to_its_grants_in_order() {
        let manifest = parse(SAMPLE).unwrap();
        let grants = compile(&manifest).unwrap();
        assert_eq!(grants.len(), 2);
        assert_eq!(grants[0].process, "shell");
        assert_eq!(grants[0].object, "console");
        assert_eq!(right_names(grants[0].rights), ["read", "write"]);
        assert_eq!(right_names(grants[1].rights), ["read", "grant"]);
    }

    #[test]
    fn an_unknown_right_is_refused_at_parse_time() {
        let text = SAMPLE.replace(
            "rights = [\"read\", \"write\"]\n\n[objects.config]",
            "rights = [\"read\", \"exec\"]\n\n[objects.config]",
        );
        assert!(matches!(
            parse(&text),
            Err(ManifestError::UnknownRight { right, .. }) if right == "exec"
        ));
    }

    #[test]
    fn a_grant_above_the_ceiling_is_refused_and_names_the_right() {
        let text = SAMPLE.replace(
            "object = \"console\", rights = [\"read\", \"write\"]",
            "object = \"console\", rights = [\"read\", \"grant\"]",
        );
        let manifest = parse(&text).unwrap();
        assert!(matches!(
            compile(&manifest),
            Err(ManifestError::AboveCeiling { right, .. }) if right == "grant"
        ));
    }

    #[test]
    fn a_grant_on_an_undeclared_object_is_refused() {
        let text = SAMPLE.replace("object = \"console\"", "object = \"disk\"");
        let manifest = parse(&text).unwrap();
        assert!(matches!(
            compile(&manifest),
            Err(ManifestError::UnknownObject { object, .. }) if object == "disk"
        ));
    }

    #[test]
    fn a_duplicate_process_is_refused() {
        let text = format!("{SAMPLE}\n[[process]]\nname = \"shell\"\n");
        assert!(matches!(parse(&text), Err(ManifestError::DuplicateProcess(n)) if n == "shell"));
    }

    #[test]
    fn lint_flags_delegation_and_unused_objects() {
        let manifest = parse(SAMPLE).unwrap();
        let findings = lint(&manifest);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].process, "loader");
        assert!(findings[0].message.contains("grant"));

        let unused = parse("[objects.spare]\nrights = [\"read\"]\n").unwrap();
        let findings = lint(&unused);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("granted to no process"));
    }

    /// Every combination of rights a process asks for, against every ceiling: the
    /// compile succeeds exactly when the request lies inside the ceiling, and each
    /// grant it makes carries exactly the rights asked for. All 8 x 8 pairs.
    #[test]
    fn property_grants_never_exceed_the_ceiling() {
        let names_of = |mask: u8| {
            ["read", "write", "grant"]
                .iter()
                .enumerate()
                .filter(|(bit, _)| mask & (1 << bit) != 0)
                .map(|(_, name)| format!("\"{name}\""))
                .collect::<Vec<_>>()
                .join(", ")
        };
        for ceiling in 0u8..8 {
            for request in 0u8..8 {
                let text = format!(
                    "[objects.o]\nrights = [{}]\n\n[[process]]\nname = \"p\"\n\
                     grants = [ {{ object = \"o\", rights = [{}] }} ]\n",
                    names_of(ceiling),
                    names_of(request)
                );
                let manifest = parse(&text).unwrap();
                let inside = request & !ceiling == 0;
                match compile(&manifest) {
                    Ok(grants) => {
                        assert!(
                            inside,
                            "ceiling {ceiling:03b} allowed request {request:03b}"
                        );
                        assert_eq!(grants.len(), 1);
                        assert_eq!(
                            right_names(grants[0].rights).len(),
                            request.count_ones() as usize
                        );
                        assert!(manifest.objects["o"].contains(grants[0].rights));
                    }
                    Err(ManifestError::AboveCeiling { .. }) => {
                        assert!(
                            !inside,
                            "ceiling {ceiling:03b} refused request {request:03b}"
                        );
                    }
                    Err(other) => panic!("unexpected error: {other}"),
                }
            }
        }
    }
}
