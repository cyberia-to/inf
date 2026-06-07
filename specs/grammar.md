---
tags: cyber, soft3, inf
crystal-type: spec
crystal-domain: cyber
---
# inf grammar (EBNF)

formal grammar for the pure register of [[inf]]. semantics in
[language](language.md). productions marked `[trident]` lower directly to a
[[trident]] grammar production; the pure subset is a sub-grammar of Trident.

```ebnf
(* Program *)
program       = decl* rule+ mutation? option* ;
decl          = pub_input | pub_output ;          (* [trident] declarations *)
pub_input     = "pub" "input" IDENT ":" type ;
pub_output    = "pub" "output" IDENT ":" type ;

(* Rules *)
rule          = head ":=" body rule_attr* ;       (* [trident] fn_def + block *)
              | entry_rule ;
entry_rule    = "?" "[" head_args "]" ":=" body rule_attr* ;
head          = IDENT "[" head_args "]" ;
head_args     = head_arg ("," head_arg)* ;
head_arg      = IDENT | aggr "(" IDENT ")" ;
aggr          = "count" | "sum" | "min" | "max" | "mean"
              | "union" | "intersection" | "collect" ;
rule_attr     = ":bounded" INTEGER ;              (* explicit recursion bound *)

(* Body — conjunction of atoms *)
body          = atom ("," atom)* ;                (* [trident] composition *)
atom          = relation_read | rule_apply | condition | bind ;
relation_read = relation "{" field_binds "}"       (* [trident] look (pattern 17) *)
              | relation "[" pos_binds "]" ;
relation      = IDENT ;
field_binds   = field_bind ("," field_bind)* ;
field_bind    = IDENT (":" term)? ;
pos_binds     = term ("," term)* ;
rule_apply    = IDENT "[" pos_binds "]" ;
condition     = call ;                             (* [trident] bin_op / call into Tri/Bt *)
bind          = IDENT "=" term ;                   (* [trident] let_stmt *)
call          = sibling "." IDENT "(" args? ")"    (* interop: Tri/Rs/Bt/Ten *)
              | builtin "(" args? ")" ;
sibling       = "Tri" | "Rs" | "Bt" | "Ten" ;
builtin       = "gt" | "lt" | "ge" | "le" | "eq" | "neq"
              | "add" | "sub" | "mul" | "not" | "in" ;
args          = term ("," term)* ;

(* Terms *)
term          = literal | IDENT | call | address | "[" args? "]" ;
address       = "#" PATH | "@" IDENT | "~" IDENT ;  (* cybermark, [trident] *)

(* Mutation — the entry rule's tuples become a cyberlink batch (a signal) *)
mutation      = (":link" | ":unlink") "{" field_binds "}"   (* canonical → signal *)
              | (":put" | ":rm") "_" IDENT "{" field_binds "}" ;  (* temp relation, pure *)

(* Options — apply to the whole program *)
option        = ":limit" INTEGER | ":offset" INTEGER
              | ":sort" ("+"|"-") IDENT
              | ":assert" ("none" | "some") ;

(* Types *)
type          = "Field" | "Word" | "Bool" | "Cid" | "Neuron"  (* [trident] *)
              | "[" type ";" INTEGER "]"
              | "{" (IDENT ":" type ",")* "}" ;

(* Literals *)
literal       = INTEGER | "true" | "false" ;
INTEGER       = [0-9]+ ;
PATH          = IDENT ("/" IDENT)* ;
IDENT         = [a-zA-Z_][a-zA-Z0-9_]* ;
comment       = "//" .* NEWLINE ;
```

## notes

- `:bounded N` is the explicit recursion bound. it maps to Trident's
  `for … bounded N`. a recursive rule without `:bounded` and without an
  inferable closed-form bound is rejected (see [language](language.md),
  bounded recursion).
- `relation_read` lowers to the [[nox]] `look` pattern; each read carries a
  [[lens]] opening against the graph root (see proof).
- `condition` and arithmetic `builtin`s lower to [[Trident|Tri]] (field) or
  [[Bt]] (bitwise); inf does not define their semantics, it delegates them.
- `address` terms are [[markup|cybermark]] sigils used as first-class values,
  the same as in Trident and [[rune]].
- floating-point literals have no production. numbers are integer literals
  interpreted as field or word atoms.

## divergence from CozoScript

the bootstrap surface (docs/queries.md) is CozoScript. this grammar narrows it
to the provable subset:

- recursion requires a static bound (`:bounded` or inferable); CozoScript
  allows unbounded fixpoint to convergence.
- arithmetic delegates to siblings; CozoScript has its own float math.
- mutations target signals, not a local store. `:link`/`:unlink` over canonical
  relations emit a staked cyberlink batch (a [[signal]]) through cybergraph →
  [[bbg]]; CozoScript's `:put`/`:rm` write a local DB. `:put`/`:rm` survive only
  for underscore-prefixed temp relations. the derivation half is in the pure
  subset; the commit is an effect.
