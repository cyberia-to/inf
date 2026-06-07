---
tags: cyber, soft3, inf
crystal-type: spec
crystal-domain: cyber
---
# inf specs — what

canonical specification for [[inf]], proof language #10 of the cyber stack
(Infer — Horn clauses, relation/inference). the declarative twin of [[rune]];
sibling of [[Rs]], [[Trident|Tri]], [[Bt]], [[Ten]].

specs/ is the source of truth. when specs/ and rs/ (implementation) disagree,
resolve in specs/ first, then propagate. docs/ explains and teaches; specs/
defines.

## the contract

inf is a proof language. its pure subset satisfies [[trident]]'s three
constitutional constraints, so an inf program lowers query → IR → [[nox]]
patterns → zheng like any Trident dialect:

1. bounded execution — recursion is bounded fixed-point iteration with a
   static depth bound. no general recursion.
2. deterministic over a fixed snapshot — same query + same graph root produce
   identical results on every node.
3. statically-known cost — `inf cost` reports cycles before execution.

## documents

| file | what |
|---|---|
| [language](language.md) | syntax, types, value model, rules, bounded recursion, the pure subset |
| [grammar](grammar.md) | formal EBNF; productions shared with Trident marked |
| [cost](cost.md) | static cost model; the bbg ↔ inf interface |
| [proof](proof.md) | proof contract: result certified against a BBG root; Lens openings (planned) |
| [ir](ir.md) | query-plan IR and Nox-pattern lowering (planned) |
| [extensions](extensions.md) | reactive (`subscribe`) and live registers; their proof contracts (planned) |
| [interop](interop.md) | calling Tri/Rs/Bt/Ten; called from Rune-pure (planned) |
| [bootstrap](bootstrap.md) | the four-stage CozoDB migration (planned) |
| [props/](props/) | design proposals (draft → accepted → implemented) |

## status

drafting. language, grammar, cost are first. the plan and sequencing live in
.claude/plans/inf-spec-plan.md.
