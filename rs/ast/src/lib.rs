//! inf AST and IR.
//!
//! The AST is what the parser produces from the pure-register surface
//! (specs/grammar.md). The IR is the stratified, ordered plan the planner
//! produces and the evaluator and the nox lowering both consume (specs/ir.md).

// ─── AST ────────────────────────────────────────────────────────────────────

/// A term: a value-producing expression in a rule body or head argument.
#[derive(Clone, Debug, PartialEq)]
pub enum Term {
    Var(String),
    Int(i64),
    Bool(bool),
    Str(String),
    /// A field-element literal (written `f<digits>` or produced by the planner).
    Field(u64),
    /// A cybermark address used as a first-class value.
    Addr(Addr),
    List(Vec<Term>),
    Call(Box<Call>),
}

/// A cybermark sigil address: `#particle`, `@neuron`, `~name`.
#[derive(Clone, Debug, PartialEq)]
pub enum Addr {
    Particle(String),
    Neuron(String),
    Name(String),
}

/// A function application: a builtin (`gt(..)`) or a sibling call (`Tri.add(..)`).
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub sibling: Option<String>, // Some("Tri") | None for builtins
    pub func: String,
    pub args: Vec<Term>,
}

/// Column/positional bindings for a relation read or a mutation.
#[derive(Clone, Debug, PartialEq)]
pub enum Binds {
    /// `{ col: term, col2, .. }` — shorthand `{col}` normalizes to `(col, Var(col))`.
    Named(Vec<(String, Term)>),
    /// `[ term, term, .. ]` — positional by column order.
    Pos(Vec<Term>),
}

/// A body atom: a conjunct of a rule body.
#[derive(Clone, Debug, PartialEq)]
pub enum Atom {
    /// Read a relation, binding its columns to terms.
    Read { rel: String, binds: Binds },
    /// Apply another rule (a derived relation) positionally.
    Apply { rule: String, args: Vec<Term> },
    /// A condition — a boolean call into a builtin or sibling.
    Cond(Call),
    /// A binding `var = term`.
    Bind { var: String, term: Term },
    /// Stratified negation: the inner atom must not hold.
    Not(Box<Atom>),
}

/// A head argument: a grouping variable or an aggregation over one.
#[derive(Clone, Debug, PartialEq)]
pub enum HeadArg {
    Var(String),
    Aggr { op: String, var: String },
}

/// A rule head. `name = None` marks the entry rule `?`.
#[derive(Clone, Debug, PartialEq)]
pub struct Head {
    pub name: Option<String>,
    pub args: Vec<HeadArg>,
}

impl Head {
    /// Whether any head argument aggregates.
    pub fn has_aggr(&self) -> bool {
        self.args.iter().any(|a| matches!(a, HeadArg::Aggr { .. }))
    }
}

/// A fixed-rule (`<~`) invocation: a built-in graph algorithm over an edges
/// relation, with named parameters and positional arguments (specs/algorithms.md).
#[derive(Clone, Debug, PartialEq)]
pub struct Fixed {
    pub algo: String,
    pub edges: String,
    pub params: Vec<(String, Term)>,
    pub pos: Vec<Term>,
}

/// A rule. Either an inline rule (`head := body`) with an optional recursion
/// bound, or a fixed rule (`head <~ Algo(..)`).
#[derive(Clone, Debug, PartialEq)]
pub struct Rule {
    pub head: Head,
    pub body: Vec<Atom>,
    pub bound: Option<u64>,
    pub fixed: Option<Fixed>,
}

/// A canonical or temp-relation mutation.
#[derive(Clone, Debug, PartialEq)]
pub enum Mutation {
    Link(Binds),
    Unlink(Binds),
    Put { rel: String, binds: Binds },
    Rm { rel: String, binds: Binds },
}

/// An I/O declaration direction.
#[derive(Clone, Debug, PartialEq)]
pub enum Dir {
    In,
    Out,
}

/// A declared type in a `pub input/output` declaration.
#[derive(Clone, Debug, PartialEq)]
pub enum Type {
    Named(String),
    Array(Box<Type>, u64),
    Record(Vec<(String, Type)>),
}

/// A program I/O declaration: `pub input graph_root : Particle`.
#[derive(Clone, Debug, PartialEq)]
pub struct Decl {
    pub dir: Dir,
    pub name: String,
    pub ty: Type,
}

/// A program-level option applied to the entry relation.
#[derive(Clone, Debug, PartialEq)]
pub enum Opt {
    Limit(u64),
    Offset(u64),
    Sort { col: String, desc: bool },
    AssertNone,
    AssertSome,
}

/// A parsed program.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Program {
    pub decls: Vec<Decl>,
    pub rules: Vec<Rule>,
    pub mutation: Option<Mutation>,
    pub opts: Vec<Opt>,
}

/// The internal name for the entry rule's relation.
pub const ENTRY: &str = "?";

impl Program {
    /// The entry rule (`?`), if present.
    pub fn entry(&self) -> Option<&Rule> {
        self.rules.iter().find(|r| r.head.name.is_none())
    }
}

// ─── IR ─────────────────────────────────────────────────────────────────────

/// A stratum: a group of rules evaluated together. Recursive strata iterate to a
/// bounded fixed point (specs/language.md, bounded recursion).
#[derive(Clone, Debug)]
pub struct Stratum {
    pub rules: Vec<Rule>,
    pub recursive: bool,
    /// The explicit `:bounded N`, if any; otherwise the evaluator uses the
    /// committed graph bound (see specs/cost.md).
    pub bound: Option<u64>,
}

/// The compiled program: strata bottom-up, plus the entry relation and the
/// program-level mutation and options.
#[derive(Clone, Debug)]
pub struct IrProgram {
    pub strata: Vec<Stratum>,
    pub entry: String,
    pub mutation: Option<Mutation>,
    pub opts: Vec<Opt>,
}

impl Atom {
    /// The relation/rule name an atom reads, if it reads one.
    pub fn rel_name(&self) -> Option<&str> {
        match self {
            Atom::Read { rel, .. } => Some(rel),
            Atom::Apply { rule, .. } => Some(rule),
            Atom::Not(inner) => inner.rel_name(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_lookup() {
        let p = Program {
            rules: vec![
                Rule {
                    head: Head { name: Some("r".into()), args: vec![HeadArg::Var("x".into())] },
                    body: vec![],
                    bound: None,
                    fixed: None,
                },
                Rule {
                    head: Head { name: None, args: vec![HeadArg::Var("x".into())] },
                    body: vec![Atom::Apply { rule: "r".into(), args: vec![Term::Var("x".into())] }],
                    bound: None,
                    fixed: None,
                },
            ],
            decls: vec![],
            mutation: None,
            opts: vec![Opt::Limit(20)],
        };
        assert!(p.entry().is_some());
        assert!(!p.entry().unwrap().head.has_aggr());
    }

    #[test]
    fn aggr_head_detected() {
        let h = Head {
            name: None,
            args: vec![HeadArg::Var("n".into()), HeadArg::Aggr { op: "count".into(), var: "p".into() }],
        };
        assert!(h.has_aggr());
    }
}
