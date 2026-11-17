// SPDX-License-Identifier: MIT OR Apache-2.0
//! Command-line parser: words, quoting, escapes, pipelines, `&&`/`||` chains,
//! `;` and `&` separators, and `<`, `>`, `>>` redirections.
//!
//! Precedence, tightest first: pipeline `|`, then `&&` and `||` (equal
//! precedence, left to right), then the list separators `;` and `&`. So
//! `a && b; c` is two list items, and `a; b && c &` is a foreground `a` and a
//! background `b && c`.

use std::string::String;
use std::vec::Vec;

/// An operator token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// `|`
    Pipe,
    /// `&&`
    AndIf,
    /// `||`
    OrIf,
    /// `;`
    Semi,
    /// `&`
    Amp,
    /// `<`
    RedirIn,
    /// `>`
    RedirOut,
    /// `>>`
    RedirAppend,
}

/// Why a command line could not be parsed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseError {
    /// A `'` or `"` was not closed.
    UnterminatedQuote,
    /// The line ends in a lone backslash.
    TrailingBackslash,
    /// A pipeline stage or chain operand has no words (for example `| a`,
    /// `a &&`, or a redirection with no command).
    EmptyCommand,
    /// A redirection operator is not followed by a word.
    MissingRedirectTarget(Op),
    /// An operator appears where the grammar does not allow it.
    UnexpectedOp(Op),
}

/// A redirection on a command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Redirect {
    /// `< file`
    Input(String),
    /// `> file`
    Output(String),
    /// `>> file`
    Append(String),
}

/// One simple command: its words and its redirections, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    /// Program name and arguments.
    pub words: Vec<String>,
    /// Redirections, in the order written.
    pub redirects: Vec<Redirect>,
}

/// Commands joined by `|`. Never empty.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pipeline {
    /// The stages, first to last.
    pub commands: Vec<Command>,
}

/// Which chain operator joins two pipelines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chain {
    /// `&&`: run the next pipeline only if the last one succeeded.
    And,
    /// `||`: run the next pipeline only if the last one failed.
    Or,
}

/// Pipelines joined by `&&` and `||`. Never empty.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AndOr {
    /// The first pipeline.
    pub first: Pipeline,
    /// Each chain operator with the pipeline that follows it.
    pub rest: Vec<(Chain, Pipeline)>,
}

/// One entry of a list: an and-or chain and how it is run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListItem {
    /// The chain.
    pub and_or: AndOr,
    /// True when ended by `&`: run in the background.
    pub background: bool,
}

/// A whole command line: zero or more list items.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct CommandLine {
    /// The items, in the order written.
    pub items: Vec<ListItem>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Tok {
    Word(String),
    Op(Op),
}

fn flush(toks: &mut Vec<Tok>, word: &mut Option<String>) {
    if let Some(w) = word.take() {
        toks.push(Tok::Word(w));
    }
}

fn tokenize(line: &str) -> Result<Vec<Tok>, ParseError> {
    let mut toks = Vec::new();
    let mut word: Option<String> = None;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' => flush(&mut toks, &mut word),
            '\'' => {
                let w = word.get_or_insert_with(String::new);
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(x) => w.push(x),
                        None => return Err(ParseError::UnterminatedQuote),
                    }
                }
            }
            '"' => {
                let w = word.get_or_insert_with(String::new);
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(e @ ('"' | '\\' | '$' | '`')) => w.push(e),
                            Some(other) => {
                                w.push('\\');
                                w.push(other);
                            }
                            None => return Err(ParseError::UnterminatedQuote),
                        },
                        Some(x) => w.push(x),
                        None => return Err(ParseError::UnterminatedQuote),
                    }
                }
            }
            '\\' => match chars.next() {
                Some(x) => word.get_or_insert_with(String::new).push(x),
                None => return Err(ParseError::TrailingBackslash),
            },
            '|' => {
                flush(&mut toks, &mut word);
                if chars.peek() == Some(&'|') {
                    chars.next();
                    toks.push(Tok::Op(Op::OrIf));
                } else {
                    toks.push(Tok::Op(Op::Pipe));
                }
            }
            '&' => {
                flush(&mut toks, &mut word);
                if chars.peek() == Some(&'&') {
                    chars.next();
                    toks.push(Tok::Op(Op::AndIf));
                } else {
                    toks.push(Tok::Op(Op::Amp));
                }
            }
            ';' => {
                flush(&mut toks, &mut word);
                toks.push(Tok::Op(Op::Semi));
            }
            '<' => {
                flush(&mut toks, &mut word);
                toks.push(Tok::Op(Op::RedirIn));
            }
            '>' => {
                flush(&mut toks, &mut word);
                if chars.peek() == Some(&'>') {
                    chars.next();
                    toks.push(Tok::Op(Op::RedirAppend));
                } else {
                    toks.push(Tok::Op(Op::RedirOut));
                }
            }
            _ => word.get_or_insert_with(String::new).push(c),
        }
    }
    flush(&mut toks, &mut word);
    Ok(toks)
}

fn parse_command(toks: &[Tok], mut i: usize) -> Result<(Command, usize), ParseError> {
    let mut command = Command {
        words: Vec::new(),
        redirects: Vec::new(),
    };
    while let Some(tok) = toks.get(i) {
        match tok {
            Tok::Word(w) => {
                command.words.push(w.clone());
                i += 1;
            }
            Tok::Op(op @ (Op::RedirIn | Op::RedirOut | Op::RedirAppend)) => {
                let target = match toks.get(i + 1) {
                    Some(Tok::Word(w)) => w.clone(),
                    _ => return Err(ParseError::MissingRedirectTarget(*op)),
                };
                command.redirects.push(match op {
                    Op::RedirIn => Redirect::Input(target),
                    Op::RedirOut => Redirect::Output(target),
                    _ => Redirect::Append(target),
                });
                i += 2;
            }
            Tok::Op(_) => break,
        }
    }
    if command.words.is_empty() {
        return Err(ParseError::EmptyCommand);
    }
    Ok((command, i))
}

fn parse_pipeline(toks: &[Tok], mut i: usize) -> Result<(Pipeline, usize), ParseError> {
    let mut commands = Vec::new();
    loop {
        let (command, next) = parse_command(toks, i)?;
        commands.push(command);
        i = next;
        if toks.get(i) == Some(&Tok::Op(Op::Pipe)) {
            i += 1;
        } else {
            break;
        }
    }
    Ok((Pipeline { commands }, i))
}

fn parse_and_or(toks: &[Tok], i: usize) -> Result<(AndOr, usize), ParseError> {
    let (first, mut i) = parse_pipeline(toks, i)?;
    let mut rest = Vec::new();
    loop {
        let chain = match toks.get(i) {
            Some(Tok::Op(Op::AndIf)) => Chain::And,
            Some(Tok::Op(Op::OrIf)) => Chain::Or,
            _ => break,
        };
        let (pipeline, next) = parse_pipeline(toks, i + 1)?;
        rest.push((chain, pipeline));
        i = next;
    }
    Ok((AndOr { first, rest }, i))
}

/// Parses a command line.
///
/// An empty or blank line gives a `CommandLine` with no items. A trailing `;`
/// or `&` is allowed; a leading or doubled separator is an error.
pub fn parse(line: &str) -> Result<CommandLine, ParseError> {
    let toks = tokenize(line)?;
    let mut items = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let (and_or, next) = parse_and_or(&toks, i)?;
        i = next;
        let background = match toks.get(i) {
            None => false,
            Some(Tok::Op(Op::Semi)) => {
                i += 1;
                false
            }
            Some(Tok::Op(Op::Amp)) => {
                i += 1;
                true
            }
            // parse_and_or stops only at a separator or the end, so these
            // arms are not reachable from valid token streams. They are errors
            // rather than panics to keep the parser total.
            Some(Tok::Op(op)) => return Err(ParseError::UnexpectedOp(*op)),
            Some(Tok::Word(_)) => return Err(ParseError::EmptyCommand),
        };
        items.push(ListItem { and_or, background });
    }
    Ok(CommandLine { items })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(c: &Command) -> Vec<&str> {
        c.words.iter().map(String::as_str).collect()
    }

    #[test]
    fn plain_words_and_blank_lines() {
        let cl = parse("  echo   hello world ").unwrap();
        assert_eq!(cl.items.len(), 1);
        let cmd = &cl.items[0].and_or.first.commands[0];
        assert_eq!(words(cmd), ["echo", "hello", "world"]);
        assert!(!cl.items[0].background);
        assert!(parse("").unwrap().items.is_empty());
        assert!(parse("   \t ").unwrap().items.is_empty());
    }

    #[test]
    fn single_quotes_are_literal() {
        let cl = parse(r#"echo 'a \ "b" $c' d"#).unwrap();
        let cmd = &cl.items[0].and_or.first.commands[0];
        assert_eq!(words(cmd), ["echo", r#"a \ "b" $c"#, "d"]);
    }

    #[test]
    fn double_quotes_and_escapes() {
        let cl = parse(r#"echo "say \"hi\"" \$x a\ b"#).unwrap();
        let cmd = &cl.items[0].and_or.first.commands[0];
        assert_eq!(words(cmd), ["echo", r#"say "hi""#, "$x", "a b"]);
        // A backslash before an ordinary character inside double quotes is kept.
        let cl = parse(r#"echo "a\nb""#).unwrap();
        assert_eq!(
            words(&cl.items[0].and_or.first.commands[0]),
            ["echo", r"a\nb"]
        );
    }

    #[test]
    fn empty_quotes_make_an_empty_word() {
        let cl = parse("printf '' x").unwrap();
        assert_eq!(
            words(&cl.items[0].and_or.first.commands[0]),
            ["printf", "", "x"]
        );
    }

    #[test]
    fn quoting_errors() {
        assert_eq!(parse("echo 'abc"), Err(ParseError::UnterminatedQuote));
        assert_eq!(parse(r#"echo "abc"#), Err(ParseError::UnterminatedQuote));
        assert_eq!(parse(r#"echo "abc\"#), Err(ParseError::UnterminatedQuote));
        assert_eq!(parse(r"echo abc\"), Err(ParseError::TrailingBackslash));
    }

    #[test]
    fn pipeline_stages() {
        let cl = parse("cat f | grep x | wc -l").unwrap();
        let p = &cl.items[0].and_or.first;
        assert_eq!(p.commands.len(), 3);
        assert_eq!(words(&p.commands[2]), ["wc", "-l"]);
    }

    #[test]
    fn redirections_in_any_position() {
        let cl = parse("> out sort < in >> log").unwrap();
        let cmd = &cl.items[0].and_or.first.commands[0];
        assert_eq!(words(cmd), ["sort"]);
        assert_eq!(
            cmd.redirects,
            [
                Redirect::Output("out".into()),
                Redirect::Input("in".into()),
                Redirect::Append("log".into()),
            ]
        );
        // A redirection can be glued to a word without spaces.
        let cl = parse("echo hi>f").unwrap();
        assert_eq!(
            cl.items[0].and_or.first.commands[0].redirects,
            [Redirect::Output("f".into())]
        );
    }

    #[test]
    fn redirection_without_target_is_an_error() {
        assert_eq!(
            parse("echo hi >"),
            Err(ParseError::MissingRedirectTarget(Op::RedirOut))
        );
        assert_eq!(
            parse("cat < | wc"),
            Err(ParseError::MissingRedirectTarget(Op::RedirIn))
        );
    }

    #[test]
    fn empty_pipeline_stage_is_an_error() {
        assert_eq!(parse("| a"), Err(ParseError::EmptyCommand));
        assert_eq!(parse("a |"), Err(ParseError::EmptyCommand));
        assert_eq!(parse("a | | b"), Err(ParseError::EmptyCommand));
        assert_eq!(parse("> out"), Err(ParseError::EmptyCommand));
    }

    #[test]
    fn and_or_chains_are_left_associative_and_tighter_than_semicolon() {
        let cl = parse("a && b || c; d").unwrap();
        assert_eq!(cl.items.len(), 2);
        let chain = &cl.items[0].and_or;
        assert_eq!(words(&chain.first.commands[0]), ["a"]);
        assert_eq!(chain.rest.len(), 2);
        assert_eq!(chain.rest[0].0, Chain::And);
        assert_eq!(chain.rest[1].0, Chain::Or);
        assert_eq!(words(&cl.items[1].and_or.first.commands[0]), ["d"]);
        assert!(!cl.items[0].background);
    }

    #[test]
    fn ampersand_backgrounds_the_chain_before_it() {
        let cl = parse("a; b && c &").unwrap();
        assert_eq!(cl.items.len(), 2);
        assert!(!cl.items[0].background);
        assert!(cl.items[1].background);
        assert_eq!(cl.items[1].and_or.rest.len(), 1);
    }

    #[test]
    fn trailing_separator_is_allowed_but_doubled_is_not() {
        assert_eq!(parse("a;").unwrap().items.len(), 1);
        assert_eq!(parse("a &").unwrap().items.len(), 1);
        assert_eq!(parse("; a"), Err(ParseError::EmptyCommand));
        assert_eq!(parse("a ; ; b"), Err(ParseError::EmptyCommand));
        assert_eq!(parse("a && ; b"), Err(ParseError::EmptyCommand));
        assert_eq!(parse("a && "), Err(ParseError::EmptyCommand));
    }

    #[test]
    fn quoted_operators_are_words() {
        let cl = parse(r#"echo "a|b" 'x;y' \&\&"#).unwrap();
        assert_eq!(cl.items.len(), 1);
        assert_eq!(
            words(&cl.items[0].and_or.first.commands[0]),
            ["echo", "a|b", "x;y", "&&"]
        );
    }
}
