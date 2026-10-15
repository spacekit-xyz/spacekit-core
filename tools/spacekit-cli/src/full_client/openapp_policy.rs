//! Policy expressions (SpaceKit App Spec v0.2 §9).
//!
//! A policy is a boolean expression over an implicit `actor` (the caller) and
//! `resource` (the entity acted on):
//!
//! ```text
//! expr    := or
//! or      := and ( "||" and )*
//! and     := not ( "&&" not )*
//! not     := "!" not | cmp
//! cmp     := operand ( ( "==" | "!=" | "<" | "<=" | ">" | ">=" ) operand
//!                    | "in" "[" operand ( "," operand )* "]" )?
//! operand := "(" expr ")" | path | 'string' | "string" | number | true | false | null
//! path    := ( "actor" | "resource" ) ( "." ident )*
//! ```
//!
//! No function calls (deferred to v0.3). This module parses the grammar,
//! reports errors for the spec validator, and compiles expressions to
//! TypeScript for the generated `server/policies.ts`.

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Expr {
    Or(Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    Cmp(Box<Expr>, CmpOp, Box<Expr>),
    In(Box<Expr>, Vec<Expr>),
    /// `actor.id` -> ("actor", ["id"]).
    Path(String, Vec<String>),
    Str(String),
    Num(String),
    Bool(bool),
    Null,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl CmpOp {
    fn ts(self) -> &'static str {
        match self {
            CmpOp::Eq => "===",
            CmpOp::Ne => "!==",
            CmpOp::Lt => "<",
            CmpOp::Le => "<=",
            CmpOp::Gt => ">",
            CmpOp::Ge => ">=",
        }
    }
    fn spec(self) -> &'static str {
        match self {
            CmpOp::Eq => "==",
            CmpOp::Ne => "!=",
            CmpOp::Lt => "<",
            CmpOp::Le => "<=",
            CmpOp::Gt => ">",
            CmpOp::Ge => ">=",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String),
    Str(String),
    Num(String),
    Op(&'static str),
    LParen,
    RParen,
    LBrack,
    RBrack,
    Comma,
    Dot,
}

fn tokenize(src: &str) -> Result<Vec<Tok>, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        let two: String = chars[i..chars.len().min(i + 2)].iter().collect();
        let op2 = match two.as_str() {
            "==" => Some("=="),
            "!=" => Some("!="),
            "<=" => Some("<="),
            ">=" => Some(">="),
            "&&" => Some("&&"),
            "||" => Some("||"),
            _ => None,
        };
        if let Some(op) = op2 {
            out.push(Tok::Op(op));
            i += 2;
            continue;
        }
        match c {
            '<' => out.push(Tok::Op("<")),
            '>' => out.push(Tok::Op(">")),
            '!' => out.push(Tok::Op("!")),
            '(' => out.push(Tok::LParen),
            ')' => out.push(Tok::RParen),
            '[' => out.push(Tok::LBrack),
            ']' => out.push(Tok::RBrack),
            ',' => out.push(Tok::Comma),
            '.' => out.push(Tok::Dot),
            '=' => return Err("`=` is not an operator; use `==`".into()),
            '&' | '|' => return Err(format!("`{c}` is not an operator; use `&&` or `||`")),
            '\'' | '"' => {
                let quote = c;
                let mut s = String::new();
                i += 1;
                while i < chars.len() && chars[i] != quote {
                    s.push(chars[i]);
                    i += 1;
                }
                if i >= chars.len() {
                    return Err("unterminated string literal".into());
                }
                out.push(Tok::Str(s));
            }
            '@' => {
                return Err(
                    "`@` references (contract reads such as `@abi.X.owner()`) are dApp policies, \
                     which this generator does not support yet"
                        .into(),
                )
            }
            d if d.is_ascii_digit() || (d == '-' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit()) => {
                let mut s = String::from(d);
                i += 1;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    s.push(chars[i]);
                    i += 1;
                }
                out.push(Tok::Num(s));
                continue;
            }
            a if a.is_ascii_alphabetic() || a == '_' => {
                let mut s = String::new();
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    s.push(chars[i]);
                    i += 1;
                }
                out.push(Tok::Ident(s));
                continue;
            }
            other => return Err(format!("unexpected character `{other}`")),
        }
        i += 1;
    }
    Ok(out)
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }
    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        self.pos += 1;
        t
    }
    fn eat_op(&mut self, op: &str) -> bool {
        if let Some(Tok::Op(o)) = self.peek() {
            if *o == op {
                self.pos += 1;
                return true;
            }
        }
        false
    }
    fn or(&mut self) -> Result<Expr, String> {
        let mut left = self.and()?;
        while self.eat_op("||") {
            let right = self.and()?;
            left = Expr::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }
    fn and(&mut self) -> Result<Expr, String> {
        let mut left = self.not()?;
        while self.eat_op("&&") {
            let right = self.not()?;
            left = Expr::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }
    fn not(&mut self) -> Result<Expr, String> {
        if self.eat_op("!") {
            return Ok(Expr::Not(Box::new(self.not()?)));
        }
        self.cmp()
    }
    fn cmp(&mut self) -> Result<Expr, String> {
        let left = self.operand()?;
        let op = match self.peek() {
            Some(Tok::Op("==")) => Some(CmpOp::Eq),
            Some(Tok::Op("!=")) => Some(CmpOp::Ne),
            Some(Tok::Op("<")) => Some(CmpOp::Lt),
            Some(Tok::Op("<=")) => Some(CmpOp::Le),
            Some(Tok::Op(">")) => Some(CmpOp::Gt),
            Some(Tok::Op(">=")) => Some(CmpOp::Ge),
            _ => None,
        };
        if let Some(op) = op {
            self.pos += 1;
            let right = self.operand()?;
            return Ok(Expr::Cmp(Box::new(left), op, Box::new(right)));
        }
        if let Some(Tok::Ident(w)) = self.peek() {
            if w == "in" {
                self.pos += 1;
                if self.next() != Some(Tok::LBrack) {
                    return Err("expected `[` after `in`".into());
                }
                let mut items = Vec::new();
                if self.peek() == Some(&Tok::RBrack) {
                    self.pos += 1;
                } else {
                    loop {
                        items.push(self.operand()?);
                        match self.next() {
                            Some(Tok::Comma) => continue,
                            Some(Tok::RBrack) => break,
                            _ => return Err("expected `,` or `]` in list".into()),
                        }
                    }
                }
                return Ok(Expr::In(Box::new(left), items));
            }
        }
        Ok(left)
    }
    fn operand(&mut self) -> Result<Expr, String> {
        match self.next() {
            Some(Tok::LParen) => {
                let e = self.or()?;
                if self.next() != Some(Tok::RParen) {
                    return Err("expected `)`".into());
                }
                Ok(e)
            }
            Some(Tok::Str(s)) => Ok(Expr::Str(s)),
            Some(Tok::Num(n)) => Ok(Expr::Num(n)),
            Some(Tok::Ident(id)) => match id.as_str() {
                "true" => Ok(Expr::Bool(true)),
                "false" => Ok(Expr::Bool(false)),
                "null" => Ok(Expr::Null),
                "actor" | "resource" => {
                    let mut segs = Vec::new();
                    while self.peek() == Some(&Tok::Dot) {
                        self.pos += 1;
                        match self.next() {
                            Some(Tok::Ident(s)) => segs.push(s),
                            _ => return Err(format!("expected a field name after `{id}.`")),
                        }
                    }
                    if self.peek() == Some(&Tok::LParen) {
                        return Err("function calls are not part of the v0.2 policy grammar".into());
                    }
                    Ok(Expr::Path(id, segs))
                }
                other => Err(format!(
                    "unknown name `{other}` (operands are `actor.…`, `resource.…`, or literals)"
                )),
            },
            Some(t) => Err(format!("unexpected token {t:?}")),
            None => Err("unexpected end of expression".into()),
        }
    }
}

/// Parse a policy expression. Built-in keywords (`public`, `authenticated`) are
/// not expressions and must be handled by the caller.
pub(crate) fn parse(src: &str) -> Result<Expr, String> {
    let toks = tokenize(src)?;
    if toks.is_empty() {
        return Err("empty policy expression".into());
    }
    let mut p = Parser { toks, pos: 0 };
    let e = p.or()?;
    if p.pos < p.toks.len() {
        return Err(format!("unexpected trailing input at token {}", p.pos + 1));
    }
    Ok(e)
}

/// Canonical text (whitespace-insensitive) for fingerprints and diffs.
pub(crate) fn canonical(e: &Expr) -> String {
    match e {
        Expr::Or(a, b) => format!("({} || {})", canonical(a), canonical(b)),
        Expr::And(a, b) => format!("({} && {})", canonical(a), canonical(b)),
        Expr::Not(a) => format!("!{}", canonical(a)),
        Expr::Cmp(a, op, b) => format!("{} {} {}", canonical(a), op.spec(), canonical(b)),
        Expr::In(a, items) => format!(
            "{} in [{}]",
            canonical(a),
            items.iter().map(canonical).collect::<Vec<_>>().join(", ")
        ),
        Expr::Path(h, segs) => {
            if segs.is_empty() {
                h.clone()
            } else {
                format!("{h}.{}", segs.join("."))
            }
        }
        Expr::Str(s) => format!("'{s}'"),
        Expr::Num(n) => n.clone(),
        Expr::Bool(b) => b.to_string(),
        Expr::Null => "null".into(),
    }
}

/// Compile to a TypeScript boolean expression over `actor` and `resource`,
/// using the `read(obj, [path])` helper emitted alongside it.
pub(crate) fn to_ts(e: &Expr) -> String {
    match e {
        Expr::Or(a, b) => format!("({} || {})", to_ts(a), to_ts(b)),
        Expr::And(a, b) => format!("({} && {})", to_ts(a), to_ts(b)),
        Expr::Not(a) => format!("!{}", to_ts(a)),
        Expr::Cmp(a, op, b) => {
            // `x == null` should also match `undefined`.
            match (op, a.as_ref(), b.as_ref()) {
                (CmpOp::Eq, _, Expr::Null) => format!("({} == null)", to_ts(a)),
                (CmpOp::Ne, _, Expr::Null) => format!("({} != null)", to_ts(a)),
                (CmpOp::Eq, Expr::Null, _) => format!("({} == null)", to_ts(b)),
                (CmpOp::Ne, Expr::Null, _) => format!("({} != null)", to_ts(b)),
                // A missing value never satisfies an equality with another missing value.
                (CmpOp::Eq, Expr::Path(..), Expr::Path(..)) => {
                    format!("same({}, {})", to_ts(a), to_ts(b))
                }
                _ => format!("({} {} {})", to_ts(a), op.ts(), to_ts(b)),
            }
        }
        Expr::In(a, items) => format!(
            "[{}].includes({} as never)",
            items.iter().map(to_ts).collect::<Vec<_>>().join(", "),
            to_ts(a)
        ),
        Expr::Path(h, segs) => {
            if segs.is_empty() {
                h.clone()
            } else {
                let parts: Vec<String> = segs.iter().map(|s| format!("\"{s}\"")).collect();
                format!("read({h}, [{}])", parts.join(", "))
            }
        }
        Expr::Str(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
        Expr::Num(n) => n.clone(),
        Expr::Bool(b) => b.to_string(),
        Expr::Null => "null".into(),
    }
}

/// Every `resource.…` path the expression reads (without the head).
pub(crate) fn resource_paths(e: &Expr, out: &mut Vec<Vec<String>>) {
    match e {
        Expr::Or(a, b) | Expr::And(a, b) | Expr::Cmp(a, _, b) => {
            resource_paths(a, out);
            resource_paths(b, out);
        }
        Expr::Not(a) => resource_paths(a, out),
        Expr::In(a, items) => {
            resource_paths(a, out);
            for i in items {
                resource_paths(i, out);
            }
        }
        Expr::Path(h, segs) if h == "resource" => {
            if !out.contains(segs) {
                out.push(segs.clone());
            }
        }
        _ => {}
    }
}

/// Whether the expression reads `resource` at all.
pub(crate) fn uses_resource(e: &Expr) -> bool {
    let mut v = Vec::new();
    resource_paths(e, &mut v);
    !v.is_empty()
}

/// A nested include tree for the relations a policy walks:
/// `resource.album.owner.id` -> { album: { owner: {} } } (leaf fields dropped).
#[derive(Debug, Default, Clone)]
pub(crate) struct IncludeTree(pub BTreeMap<String, IncludeTree>);

impl IncludeTree {
    /// Render as a Prisma `include` object literal, or `None` when empty.
    pub fn to_prisma(&self) -> Option<String> {
        if self.0.is_empty() {
            return None;
        }
        let parts: Vec<String> = self
            .0
            .iter()
            .map(|(k, v)| match v.to_prisma() {
                Some(inner) => format!("{k}: {{ include: {inner} }}"),
                None => format!("{k}: true"),
            })
            .collect();
        Some(format!("{{ {} }}", parts.join(", ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_spec_examples() {
        for src in [
            "actor.id == resource.buyer.id",
            "actor.role == 'admin'",
            "actor.role == 'staff' || actor.id == resource.buyer.id",
            "resource.album.visibility == 'public' || actor.id == resource.album.owner.id",
            "actor.role in ['admin', 'staff'] && !(resource.locked == true)",
            "actor.wallet != null",
        ] {
            parse(src).unwrap_or_else(|e| panic!("{src}: {e}"));
        }
    }

    #[test]
    fn rejects_out_of_grammar() {
        assert!(parse("actor.id = resource.id").is_err());
        assert!(parse("user.id == resource.id").is_err());
        assert!(parse("hasRole(actor, 'admin')").is_err());
        assert!(parse("actor.wallet == @abi.Bookshop.owner()").is_err());
        assert!(parse("actor.id ==").is_err());
        assert!(parse("").is_err());
    }

    #[test]
    fn compiles_to_typescript() {
        let e = parse("actor.role == 'staff' || actor.id == resource.buyer.id").unwrap();
        assert_eq!(
            to_ts(&e),
            "((read(actor, [\"role\"]) === \"staff\") || same(read(actor, [\"id\"]), read(resource, [\"buyer\", \"id\"])))"
        );
        let e = parse("actor.role in ['admin', 'staff']").unwrap();
        assert_eq!(to_ts(&e), "[\"admin\", \"staff\"].includes(read(actor, [\"role\"]) as never)");
    }

    #[test]
    fn canonical_ignores_whitespace() {
        let a = parse("actor.id==resource.owner.id").unwrap();
        let b = parse("  actor.id  ==  resource.owner.id ").unwrap();
        assert_eq!(canonical(&a), canonical(&b));
    }

    #[test]
    fn collects_resource_paths() {
        let e = parse("resource.album.visibility == 'public' || actor.id == resource.album.owner.id").unwrap();
        let mut v = Vec::new();
        resource_paths(&e, &mut v);
        assert_eq!(v, vec![vec!["album".to_string(), "visibility".into()], vec!["album".into(), "owner".into(), "id".into()]]);
    }
}
