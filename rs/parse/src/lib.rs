//! Recursive-descent parser for the inf pure register (specs/grammar.md).
//!
//! A program is rules, then an optional mutation, then options. Conditions are
//! function-call form (`gt(a, b)`, `Tri.mul(x, y)`); arithmetic is infix
//! (`+ - *`) inside expressions; aggregation lives in the head (`?[n, count(p)]`).

use inf_ast::*;
use inf_lex::{lex, Spanned, Tok};

#[derive(Clone, Debug, PartialEq)]
pub struct ParseError {
    pub msg: String,
    pub line: usize,
    pub col: usize,
}

pub fn parse(src: &str) -> Result<Program, ParseError> {
    let toks = lex(src).map_err(|e| ParseError { msg: e.msg, line: e.line, col: e.col })?;
    let mut p = Parser { toks, pos: 0 };
    p.program()
}

struct Parser {
    toks: Vec<Spanned>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos).map(|s| &s.tok)
    }
    fn peek2(&self) -> Option<&Tok> {
        self.toks.get(self.pos + 1).map(|s| &s.tok)
    }
    fn loc(&self) -> (usize, usize) {
        self.toks
            .get(self.pos)
            .map(|s| (s.line, s.col))
            .unwrap_or((0, 0))
    }
    fn err<T>(&self, msg: impl Into<String>) -> Result<T, ParseError> {
        let (line, col) = self.loc();
        Err(ParseError { msg: msg.into(), line, col })
    }
    fn bump(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).map(|s| s.tok.clone());
        self.pos += 1;
        t
    }
    fn eat(&mut self, t: &Tok) -> bool {
        if self.peek() == Some(t) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn expect(&mut self, t: &Tok) -> Result<(), ParseError> {
        if self.eat(t) {
            Ok(())
        } else {
            self.err(format!("expected {t:?}, found {:?}", self.peek()))
        }
    }
    fn ident(&mut self) -> Result<String, ParseError> {
        match self.bump() {
            Some(Tok::Ident(s)) => Ok(s),
            other => self.err_back(format!("expected identifier, found {other:?}")),
        }
    }
    fn err_back<T>(&self, msg: String) -> Result<T, ParseError> {
        let (line, col) = self
            .toks
            .get(self.pos.saturating_sub(1))
            .map(|s| (s.line, s.col))
            .unwrap_or((0, 0));
        Err(ParseError { msg, line, col })
    }

    fn program(&mut self) -> Result<Program, ParseError> {
        let mut decls = Vec::new();
        while self.peek() == Some(&Tok::Ident("pub".into())) {
            decls.push(self.decl()?);
        }
        let mut rules = Vec::new();
        while self.starts_rule() {
            rules.push(self.rule()?);
        }
        let mut mutation = None;
        let mut opts = Vec::new();
        let mut subscribe = None;
        while self.peek() == Some(&Tok::Colon) {
            self.bump();
            let name = self.ident()?;
            match name.as_str() {
                "link" => mutation = Some(Mutation::Link(self.named_binds()?)),
                "unlink" => mutation = Some(Mutation::Unlink(self.named_binds()?)),
                "put" => {
                    let rel = self.ident()?;
                    mutation = Some(Mutation::Put { rel, binds: self.named_binds()? });
                }
                "rm" => {
                    let rel = self.ident()?;
                    mutation = Some(Mutation::Rm { rel, binds: self.named_binds()? });
                }
                "limit" => opts.push(Opt::Limit(self.uint()?)),
                "offset" => opts.push(Opt::Offset(self.uint()?)),
                "sort" | "order" => {
                    let desc = if self.eat(&Tok::Minus) {
                        true
                    } else {
                        self.eat(&Tok::Plus);
                        false
                    };
                    opts.push(Opt::Sort { col: self.ident()?, desc });
                }
                "assert" => {
                    let kind = self.ident()?;
                    match kind.as_str() {
                        "none" => opts.push(Opt::AssertNone),
                        "some" => opts.push(Opt::AssertSome),
                        _ => return self.err(format!("expected none|some after :assert, found {kind}")),
                    }
                }
                "subscribe" => {
                    let rel = self.ident()?;
                    let binds = if self.peek() == Some(&Tok::LBrace) {
                        self.named_binds()?
                    } else {
                        Binds::Named(vec![])
                    };
                    subscribe = Some((rel, binds));
                }
                other => return self.err(format!("unknown directive :{other}")),
            }
        }
        if self.pos < self.toks.len() {
            return self.err(format!("unexpected trailing token {:?}", self.peek()));
        }
        Ok(Program { decls, rules, mutation, opts, subscribe })
    }

    fn decl(&mut self) -> Result<Decl, ParseError> {
        self.expect_ident("pub")?;
        let dir = match self.ident()?.as_str() {
            "input" => Dir::In,
            "output" => Dir::Out,
            other => return self.err(format!("expected input|output after `pub`, found {other}")),
        };
        let name = self.ident()?;
        self.expect(&Tok::Colon)?;
        let ty = self.ty()?;
        Ok(Decl { dir, name, ty })
    }

    fn expect_ident(&mut self, want: &str) -> Result<(), ParseError> {
        let got = self.ident()?;
        if got == want {
            Ok(())
        } else {
            self.err(format!("expected `{want}`, found `{got}`"))
        }
    }

    fn ty(&mut self) -> Result<Type, ParseError> {
        match self.peek().cloned() {
            Some(Tok::LBracket) => {
                self.bump();
                let inner = self.ty()?;
                self.expect(&Tok::Semi)?;
                let n = self.uint()?;
                self.expect(&Tok::RBracket)?;
                Ok(Type::Array(Box::new(inner), n))
            }
            Some(Tok::LBrace) => {
                self.bump();
                let mut fields = Vec::new();
                if self.peek() != Some(&Tok::RBrace) {
                    loop {
                        let name = self.ident()?;
                        self.expect(&Tok::Colon)?;
                        fields.push((name, self.ty()?));
                        if !self.eat(&Tok::Comma) {
                            break;
                        }
                    }
                }
                self.expect(&Tok::RBrace)?;
                Ok(Type::Record(fields))
            }
            Some(Tok::Ident(_)) => Ok(Type::Named(self.ident()?)),
            other => self.err(format!("expected a type, found {other:?}")),
        }
    }

    fn starts_rule(&self) -> bool {
        match self.peek() {
            Some(Tok::Question) => true,
            Some(Tok::Ident(_)) => self.peek2() == Some(&Tok::LBracket),
            _ => false,
        }
    }

    fn uint(&mut self) -> Result<u64, ParseError> {
        match self.bump() {
            Some(Tok::Int(i)) if i >= 0 => Ok(i as u64),
            other => self.err_back(format!("expected non-negative integer, found {other:?}")),
        }
    }

    fn rule(&mut self) -> Result<Rule, ParseError> {
        let head = self.head()?;
        if self.eat(&Tok::FixedArrow) {
            let fixed = self.fixed_call()?;
            return Ok(Rule { head, body: vec![], bound: None, fixed: Some(fixed) });
        }
        self.expect(&Tok::Assign)?;
        let body = self.body()?;
        let mut bound = None;
        // a trailing `:bounded N` attaches to this rule
        if self.peek() == Some(&Tok::Colon) && self.peek2() == Some(&Tok::Ident("bounded".into())) {
            self.bump();
            self.bump();
            bound = Some(self.uint()?);
        }
        Ok(Rule { head, body, bound, fixed: None })
    }

    /// `Algo( edges[], key: term, posarg, .. )`
    fn fixed_call(&mut self) -> Result<Fixed, ParseError> {
        let algo = self.ident()?;
        self.expect(&Tok::LParen)?;
        let mut edges = None;
        let mut params = Vec::new();
        let mut pos = Vec::new();
        if self.peek() != Some(&Tok::RParen) {
            loop {
                let is_rel = matches!(self.peek(), Some(Tok::Ident(_)))
                    && self.peek2() == Some(&Tok::LBracket);
                let is_param = matches!(self.peek(), Some(Tok::Ident(_)))
                    && self.peek2() == Some(&Tok::Colon);
                if is_rel {
                    let name = self.ident()?;
                    self.expect(&Tok::LBracket)?;
                    self.expect(&Tok::RBracket)?;
                    if edges.is_none() {
                        edges = Some(name);
                    }
                } else if is_param {
                    let key = self.ident()?;
                    self.expect(&Tok::Colon)?;
                    params.push((key, self.expr()?));
                } else {
                    pos.push(self.expr()?);
                }
                if !self.eat(&Tok::Comma) {
                    break;
                }
            }
        }
        self.expect(&Tok::RParen)?;
        let edges = edges.ok_or_else(|| ParseError {
            msg: format!("fixed rule `{algo}` needs an edges relation argument, e.g. `edges[]`"),
            line: 0,
            col: 0,
        })?;
        Ok(Fixed { algo, edges, params, pos })
    }

    fn head(&mut self) -> Result<Head, ParseError> {
        let name = if self.eat(&Tok::Question) {
            None
        } else {
            Some(self.ident()?)
        };
        self.expect(&Tok::LBracket)?;
        let mut args = Vec::new();
        if self.peek() != Some(&Tok::RBracket) {
            loop {
                let id = self.ident()?;
                if self.eat(&Tok::LParen) {
                    let var = self.ident()?;
                    self.expect(&Tok::RParen)?;
                    args.push(HeadArg::Aggr { op: id, var });
                } else {
                    args.push(HeadArg::Var(id));
                }
                if !self.eat(&Tok::Comma) {
                    break;
                }
            }
        }
        self.expect(&Tok::RBracket)?;
        Ok(Head { name, args })
    }

    fn body(&mut self) -> Result<Vec<Atom>, ParseError> {
        let mut atoms = vec![self.atom()?];
        while self.eat(&Tok::Comma) {
            atoms.push(self.atom()?);
        }
        Ok(atoms)
    }

    fn atom(&mut self) -> Result<Atom, ParseError> {
        if self.peek() == Some(&Tok::Ident("not".into())) {
            self.bump();
            return Ok(Atom::Not(Box::new(self.atom_simple()?)));
        }
        self.atom_simple()
    }

    fn atom_simple(&mut self) -> Result<Atom, ParseError> {
        let name = self.ident()?;
        match self.peek() {
            Some(Tok::LBrace) => Ok(Atom::Read { rel: name, binds: self.named_binds()? }),
            Some(Tok::LBracket) => Ok(Atom::Read { rel: name, binds: self.pos_binds()? }),
            Some(Tok::Eq) => {
                self.bump();
                Ok(Atom::Bind { var: name, term: self.expr()? })
            }
            Some(Tok::LParen) => Ok(Atom::Cond(self.call_tail(None, name)?)),
            Some(Tok::Dot) => {
                self.bump();
                let func = self.ident()?;
                Ok(Atom::Cond(self.call_tail(Some(name), func)?))
            }
            Some(Tok::Ident(kw)) if kw == "in" => {
                self.bump();
                let coll = self.expr()?;
                Ok(Atom::Cond(Call {
                    sibling: None,
                    func: "in".into(),
                    args: vec![Term::Var(name), coll],
                }))
            }
            other => self.err(format!("unexpected token in atom: {other:?}")),
        }
    }

    /// Parse `( args )` after a function name.
    fn call_tail(&mut self, sibling: Option<String>, func: String) -> Result<Call, ParseError> {
        self.expect(&Tok::LParen)?;
        let mut args = Vec::new();
        if self.peek() != Some(&Tok::RParen) {
            loop {
                args.push(self.expr()?);
                if !self.eat(&Tok::Comma) {
                    break;
                }
            }
        }
        self.expect(&Tok::RParen)?;
        Ok(Call { sibling, func, args })
    }

    fn named_binds(&mut self) -> Result<Binds, ParseError> {
        self.expect(&Tok::LBrace)?;
        let mut binds = Vec::new();
        if self.peek() != Some(&Tok::RBrace) {
            loop {
                let col = self.ident()?;
                let term = if self.eat(&Tok::Colon) {
                    self.expr()?
                } else {
                    Term::Var(col.clone())
                };
                binds.push((col, term));
                if !self.eat(&Tok::Comma) {
                    break;
                }
            }
        }
        self.expect(&Tok::RBrace)?;
        Ok(Binds::Named(binds))
    }

    fn pos_binds(&mut self) -> Result<Binds, ParseError> {
        self.expect(&Tok::LBracket)?;
        let mut terms = Vec::new();
        if self.peek() != Some(&Tok::RBracket) {
            loop {
                terms.push(self.expr()?);
                if !self.eat(&Tok::Comma) {
                    break;
                }
            }
        }
        self.expect(&Tok::RBracket)?;
        Ok(Binds::Pos(terms))
    }

    // ── expressions: + - (low) then * (high) then primary ──
    fn expr(&mut self) -> Result<Term, ParseError> {
        let mut left = self.mul()?;
        loop {
            let func = match self.peek() {
                Some(Tok::Plus) => "add",
                Some(Tok::Minus) => "sub",
                _ => break,
            };
            self.bump();
            let right = self.mul()?;
            left = Term::Call(Box::new(Call { sibling: None, func: func.into(), args: vec![left, right] }));
        }
        Ok(left)
    }

    fn mul(&mut self) -> Result<Term, ParseError> {
        let mut left = self.primary()?;
        while self.peek() == Some(&Tok::Star) {
            self.bump();
            let right = self.primary()?;
            left = Term::Call(Box::new(Call { sibling: None, func: "mul".into(), args: vec![left, right] }));
        }
        Ok(left)
    }

    fn primary(&mut self) -> Result<Term, ParseError> {
        match self.peek().cloned() {
            Some(Tok::Int(i)) => {
                self.bump();
                Ok(Term::Int(i))
            }
            Some(Tok::Minus) => {
                self.bump();
                match self.bump() {
                    Some(Tok::Int(i)) => Ok(Term::Int(-i)),
                    other => self.err_back(format!("expected integer after '-', found {other:?}")),
                }
            }
            Some(Tok::Str(s)) => {
                self.bump();
                Ok(Term::Str(s))
            }
            Some(Tok::Hash) => {
                self.bump();
                let mut path = self.ident()?;
                while self.eat(&Tok::Slash) {
                    path.push('/');
                    path.push_str(&self.ident()?);
                }
                Ok(Term::Addr(Addr::Particle(path)))
            }
            Some(Tok::At) => {
                self.bump();
                Ok(Term::Addr(Addr::Neuron(self.ident()?)))
            }
            Some(Tok::Tilde) => {
                self.bump();
                Ok(Term::Addr(Addr::Name(self.ident()?)))
            }
            Some(Tok::LParen) => {
                self.bump();
                let e = self.expr()?;
                self.expect(&Tok::RParen)?;
                Ok(e)
            }
            Some(Tok::LBracket) => {
                self.bump();
                let mut items = Vec::new();
                if self.peek() != Some(&Tok::RBracket) {
                    loop {
                        items.push(self.expr()?);
                        if !self.eat(&Tok::Comma) {
                            break;
                        }
                    }
                }
                self.expect(&Tok::RBracket)?;
                Ok(Term::List(items))
            }
            Some(Tok::Ident(id)) => {
                self.bump();
                match id.as_str() {
                    "true" => Ok(Term::Bool(true)),
                    "false" => Ok(Term::Bool(false)),
                    _ => match self.peek() {
                        Some(Tok::LParen) => Ok(Term::Call(Box::new(self.call_tail(None, id)?))),
                        Some(Tok::Dot) => {
                            self.bump();
                            let func = self.ident()?;
                            Ok(Term::Call(Box::new(self.call_tail(Some(id), func)?)))
                        }
                        _ => Ok(Term::Var(id)),
                    },
                }
            }
            other => self.err(format!("unexpected token in expression: {other:?}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertical_slice() {
        let p = parse(
            "?[to, score] := axons{from: #seed, to}, focus{particle: to, score}, gt(score, 5)\n:sort -score\n:limit 20",
        )
        .unwrap();
        assert_eq!(p.rules.len(), 1);
        let r = &p.rules[0];
        assert_eq!(r.head.name, None);
        assert_eq!(r.body.len(), 3);
        assert!(matches!(r.body[2], Atom::Cond(_)));
        assert_eq!(p.opts, vec![Opt::Sort { col: "score".into(), desc: true }, Opt::Limit(20)]);
    }

    #[test]
    fn bounded_recursion() {
        let p = parse(
            "reachable[p] := axons{from: #seed, to: p}\nreachable[p] := reachable[mid], axons{from: mid, to: p}\n?[p] := reachable[p]\n:bounded 5",
        );
        // :bounded after the last rule body attaches to that rule
        let p = p.unwrap();
        assert_eq!(p.rules.len(), 3);
        assert_eq!(p.rules[2].bound, Some(5));
    }

    #[test]
    fn aggregation_head() {
        let p = parse("?[neuron, count(to)] := cyberlinks{neuron, to}").unwrap();
        assert!(p.rules[0].head.has_aggr());
    }

    #[test]
    fn mutation_link() {
        let p = parse(
            "?[from] := axons{from, to: #old}, cyberlinks{neuron: @me, from}\n:link { neuron: @me, from, to: #new }",
        )
        .unwrap();
        assert!(matches!(p.mutation, Some(Mutation::Link(_))));
    }

    #[test]
    fn assert_none() {
        let p = parse("?[n, k] := karma{neuron: n, k}, lt(k, 0)\n:assert none").unwrap();
        assert_eq!(p.opts, vec![Opt::AssertNone]);
    }

    #[test]
    fn negation_and_bind() {
        let p = parse("?[p] := focus{particle: p, score}, not axons{from: #t, to: p}, x = score").unwrap();
        let b = &p.rules[0].body;
        assert!(matches!(b[1], Atom::Not(_)));
        assert!(matches!(b[2], Atom::Bind { .. }));
    }

    #[test]
    fn declarations() {
        let p = parse(
            "pub input graph_root : Particle\npub output result : Relation\n?[x] := axons{from: x, to: x}",
        )
        .unwrap();
        assert_eq!(p.decls.len(), 2);
        assert_eq!(p.decls[0].dir, Dir::In);
        assert_eq!(p.decls[0].name, "graph_root");
        assert_eq!(p.decls[1].name, "result");
        assert_eq!(p.rules.len(), 1);
    }

    #[test]
    fn declaration_types() {
        let p = parse(
            "pub input ks : [Field; 4]\npub input params : { a: Field, b: Particle }\n?[x] := r{col: x}",
        )
        .unwrap();
        assert!(matches!(p.decls[0].ty, Type::Array(_, 4)));
        assert!(matches!(p.decls[1].ty, Type::Record(_)));
    }

    #[test]
    fn sibling_call_and_infix() {
        let p = parse("?[x] := f{x}, y = Tri.mul(x, 2), gt(y, x)").unwrap();
        let b = &p.rules[0].body;
        if let Atom::Bind { term: Term::Call(c), .. } = &b[1] {
            assert_eq!(c.sibling.as_deref(), Some("Tri"));
        } else {
            panic!("expected sibling call bind");
        }
    }
}
