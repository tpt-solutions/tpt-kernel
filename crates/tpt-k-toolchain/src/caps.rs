//! The capability inspector: reads the `cap:` lines that the kernel prints at boot
//! between `caps: begin` and `caps: end`, and renders the capability graph as text
//! or as Graphviz DOT.
//!
//! Each line names a capability by table and slot, its object, its rights (`R`,
//! `W`, `G`, or `-` for each), its parent slot in the same table (a derivation),
//! and its origin `table.slot` when it is a copy granted from another table.

use std::fmt::Write as _;

/// One capability from the kernel's dump.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapRecord {
    pub table: usize,
    pub slot: usize,
    pub object: u32,
    /// Three characters: `R`, `W` and `G`, each replaced by `-` if absent.
    pub rights: String,
    /// The slot of the capability this one was derived from, in the same table.
    pub parent: Option<usize>,
    /// The `(table, slot)` this capability was copied from, if it is a copy.
    pub origin: Option<(usize, usize)>,
}

/// Reads every `cap:` line in `text`. Other lines are ignored, so the whole serial
/// transcript can be given.
///
/// # Errors
///
/// Returns the line number and reason for a `cap:` line that does not parse.
pub fn parse(text: &str) -> Result<Vec<CapRecord>, String> {
    let mut records = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let line = line.trim_end();
        let Some(fields) = line.strip_prefix("cap: ") else {
            continue;
        };
        records
            .push(parse_fields(fields).map_err(|e| format!("line {}: {e}: {line}", number + 1))?);
    }
    Ok(records)
}

fn parse_fields(fields: &str) -> Result<CapRecord, String> {
    let mut table = None;
    let mut slot = None;
    let mut object = None;
    let mut rights = None;
    let mut parent = None;
    let mut origin = None;
    for field in fields.split_whitespace() {
        let (key, value) = field
            .split_once('=')
            .ok_or_else(|| format!("field `{field}` has no value"))?;
        match key {
            "table" => table = Some(number(value)?),
            "slot" => slot = Some(number(value)?),
            "object" => object = Some(u32::try_from(number(value)?).map_err(|e| e.to_string())?),
            "rights" => {
                if value.len() != 3 || !value.chars().all(|c| "RWG-".contains(c)) {
                    return Err(format!("bad rights `{value}`"));
                }
                rights = Some(value.to_string());
            }
            "parent" => parent = optional(value, number)?,
            "origin" => origin = optional(value, parse_origin)?,
            other => return Err(format!("unknown field `{other}`")),
        }
    }
    Ok(CapRecord {
        table: table.ok_or("no table")?,
        slot: slot.ok_or("no slot")?,
        object: object.ok_or("no object")?,
        rights: rights.ok_or("no rights")?,
        parent,
        origin,
    })
}

fn number(text: &str) -> Result<usize, String> {
    text.parse()
        .map_err(|_| format!("`{text}` is not a number"))
}

fn optional<T>(text: &str, parse: impl Fn(&str) -> Result<T, String>) -> Result<Option<T>, String> {
    if text == "-" {
        Ok(None)
    } else {
        parse(text).map(Some)
    }
}

fn parse_origin(text: &str) -> Result<(usize, usize), String> {
    let (table, slot) = text
        .split_once('.')
        .ok_or_else(|| format!("origin `{text}` is not table.slot"))?;
    Ok((number(table)?, number(slot)?))
}

/// Renders the graph as text, one block per table.
#[must_use]
pub fn render_text(records: &[CapRecord]) -> String {
    let mut out = String::new();
    let mut tables: Vec<usize> = records.iter().map(|r| r.table).collect();
    tables.sort_unstable();
    tables.dedup();
    for table in tables {
        let _ = writeln!(out, "table {table}");
        for record in records.iter().filter(|r| r.table == table) {
            let _ = write!(
                out,
                "  slot {:<2} object {:<4} rights {}",
                record.slot, record.object, record.rights
            );
            if let Some(parent) = record.parent {
                let _ = write!(out, "  derived from slot {parent}");
            }
            if let Some((origin_table, origin_slot)) = record.origin {
                let _ = write!(out, "  copy of table {origin_table} slot {origin_slot}");
            }
            out.push('\n');
        }
    }
    out
}

/// Renders the graph as Graphviz DOT. Derivations within a table are solid edges
/// from parent to child, and copies across tables are dashed edges from origin to
/// copy. Nodes are grouped by table.
#[must_use]
pub fn render_dot(records: &[CapRecord]) -> String {
    let mut out = String::from("digraph caps {\n  rankdir=LR;\n  node [shape=box];\n");
    let mut tables: Vec<usize> = records.iter().map(|r| r.table).collect();
    tables.sort_unstable();
    tables.dedup();
    for table in &tables {
        let _ = writeln!(
            out,
            "  subgraph cluster_t{table} {{\n    label=\"table {table}\";"
        );
        for record in records.iter().filter(|r| r.table == *table) {
            let _ = writeln!(
                out,
                "    {} [label=\"slot {}\\nobject {} {}\"];",
                node(record.table, record.slot),
                record.slot,
                record.object,
                record.rights
            );
        }
        out.push_str("  }\n");
    }
    for record in records {
        let here = node(record.table, record.slot);
        if let Some(parent) = record.parent {
            let _ = writeln!(out, "  {} -> {here};", node(record.table, parent));
        }
        if let Some((table, slot)) = record.origin {
            let _ = writeln!(out, "  {} -> {here} [style=dashed];", node(table, slot));
        }
    }
    out.push_str("}\n");
    out
}

fn node(table: usize, slot: usize) -> String {
    format!("t{table}s{slot}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
memory: 51993 free frames
caps: begin
cap: table=0 slot=0 object=3 rights=RWG parent=- origin=-
cap: table=0 slot=1 object=256 rights=RWG parent=- origin=-
cap: table=4 slot=0 object=0 rights=RWG parent=- origin=-
cap: table=4 slot=2 object=0 rights=-W- parent=0 origin=-
cap: table=7 slot=0 object=256 rights=RWG parent=- origin=0.1
caps: end
";

    #[test]
    fn parses_the_kernel_lines_and_skips_the_rest() {
        let records = parse(SAMPLE).unwrap();
        assert_eq!(records.len(), 5);
        assert_eq!(records[3].parent, Some(0));
        assert_eq!(records[4].origin, Some((0, 1)));
        assert_eq!(records[4].rights, "RWG");
        assert_eq!(records[0].origin, None);
    }

    #[test]
    fn a_bad_line_is_reported_with_its_number() {
        let err = parse("cap: table=0 slot=0 object=x rights=RWG parent=- origin=-\n").unwrap_err();
        assert!(err.starts_with("line 1:"), "{err}");
        let err = parse("cap: table=0 slot=0 object=1 rights=RQG parent=- origin=-\n").unwrap_err();
        assert!(err.contains("bad rights"), "{err}");
    }

    #[test]
    fn text_shows_tables_derivations_and_copies() {
        let text = render_text(&parse(SAMPLE).unwrap());
        assert!(text.contains("table 4"));
        assert!(text.contains("derived from slot 0"));
        assert!(text.contains("copy of table 0 slot 1"));
    }

    #[test]
    fn dot_has_a_node_per_capability_and_an_edge_per_link() {
        let dot = render_dot(&parse(SAMPLE).unwrap());
        assert!(dot.starts_with("digraph caps {"));
        assert!(dot.contains("t4s0 -> t4s2;"));
        assert!(dot.contains("t0s1 -> t7s0 [style=dashed];"));
        assert!(dot.contains("cluster_t7"));
        assert!(dot.trim_end().ends_with('}'));
    }
}
