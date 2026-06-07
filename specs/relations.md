---
tags: cyber, soft3, inf
crystal-type: spec
crystal-domain: cyber
---
# inf relations

how [[inf]] sees, scopes, reads, and appends to the cybergraph relations. the
canonical relation schemas (columns, keys, types) live in cybergraph/specs/query.md
— this file defines inf's use of them, not their shape. inf owns no storage; the
relations are views over committed [[bbg]] state.

## the relation set

| relation | role | inf access |
|---|---|---|
| `cyberlinks` | the base assertions | read (scoped); append via `:link`/`:unlink` |
| `particles` | nodes; enter by being linked (A4) | read |
| `neurons` | agents — focus, karma, stake | read |
| `signals` | finalized assertion batches | read |
| `axons` | derived aggregate weight between two particles | read |
| `focus` | φ* per particle, from [[tru]] | read |
| `karma` | per neuron, from [[tru]] | read |

types follow the canonical schema: content-addressed identifiers are `Bytes`
(32-byte digests), counts and amounts are `Int`. inf has no floating point;
`focus` and `karma` are field elements as committed (see [language](language.md)).

## what is base and what is derived

only `cyberlinks` is written. every other relation is a function of the
cyberlinks and signals:

- `particles` appear when a cyberlink references them (A4).
- `axons` aggregate the cyberlinks between two particles (A6).
- `focus`, `karma` are computed by [[tru]] and committed back into bbg.
- `signals` record which batches finalized.

so inf's mutation surface is a single base relation — `cyberlinks` — appended as
assertions or withdrawals. the derived relations recompute as a consequence; inf
never writes them directly. writing `axons`, `focus`, or `karma` is rejected.

## visibility and scope

- `axons` — public aggregate. any caller scans it.
- `cyberlinks` — neuron-scoped. a scan returns the caller's own links (plaintext,
  from the personal owner store) and links the caller holds an opening key for; a
  bare public scan returns nothing. individual cyberlinks are private (the
  commitment polynomial A(x) in [[bbg]]); the public face is `axons`.
- `particles`, `neurons`, `signals`, `focus`, `karma` — public committed state.

`axons` answer "what does the graph say"; `cyberlinks` answer "what did I assert."

## read model

reads resolve against a committed snapshot at a graph root. a read lowers to the
[[nox]] `look` pattern and carries a [[lens]] opening against the root, so each
returned row is provable (see proof). consistency follows cybergraph/specs/query.md:
`cyberlinks`, `particles`, `neurons`, `signals` are consistent at the latest
accepted height; `axons` updates in the same transaction; `focus` and `karma` lag
by one tru cycle.

a query may name relations from the source it is bound to: the bbg source
exposes the raw committed relations; the cybergraph source adds
derived/normalized relations, tru-fresh `focus`/`karma`, and particle content
(see the source boundary below).

## mutation — append only

the cybergraph is append-only (axiom A3). inf mutates by appending cyberlinks:

- `:link { … }` — append the derived set of cyberlinks (assertions).
- `:unlink { … }` — append a stake withdrawal over the derived set; conviction
  drops and the live `axons` shift, while the original links persist in history.

both compile to one [[signal]] — a staked, signed batch — committed through
cybergraph → bbg. canonical relations are mutated only as signals. the
derivation of the set is pure and provable; the commit is the staked effect (see
[language](language.md), mutation).

## temp relations

relations prefixed with `_` are query-local: they exist only for the duration of
a query and never reach the graph. `:put`/`:rm` write them; they are pure
intermediates, the only relations inf writes outside the signal path. they live
as [[nox]] nouns, not in a separate store.

## mutable views — filesystem and database

move, rename, delete, update, upsert are notions layered on top of the
append-only graph, not relations of it. inf realizes their bulk forms as appends:
supersession (the latest assertion under a name wins — name resolution is the
latest cyberlink by neuron and path), withdrawal (`:unlink`), and tombstones. a
filesystem or database view sees mutable records; underneath, `cyberlinks` only
ever grows, and every past state stays queryable through the time dimension.

## the source boundary

inf's evaluator resolves every base relation through one boundary, so the same
engine runs over local, bbg, or cybergraph data unchanged:

```rust
trait RelationSource {
    fn schema(&self, name: &str) -> Option<RelationSchema>;
    fn get(&self, name: &str, key: &Tuple) -> Result<Option<Tuple>>;
    fn scan(&self, name: &str, range: NamespaceRange) -> TupleIter;
    fn snapshot(&self) -> Option<Height>;   // None = local, no chain time
    fn provable(&self) -> bool;             // reads can become Lens openings
    fn writable(&self, name: &str) -> bool; // canonical relations: false
}
```

a session binds one source at open: the local source (bootstrap dev harness, no
proofs), the bbg source (committed state, the pure register, provable), or the
cybergraph source (bbg plus derived relations, tru freshness, content, and the
reactive register). these correspond to the registers in [language](language.md)
and [extensions](extensions.md). a query-local temp store is always present
alongside the bound source, holding intermediate and `_`-prefixed relations.

## temp store allocation

the temp store is inf's own ordered-KV/MVCC structure, allocated from honeycrisp
`unimem` (an arena/pool; a heap arena off Apple Silicon) — the same memory the
graph-algorithm working sets use, zero-copy to the GPU/AMX that run them. inf owns
the structure, honeycrisp owns the memory, bbg owns the state. bbg's typed
EPHEMERAL dimension is a sibling over the same allocator, not a host for query
scratch.

## open

- mode discovery: a query declares no mode; the host binds the source, and a
  query that needs more than the source offers (e.g. content under the bbg source)
  fails with a capability error.
- owner-key model: how the caller's identity and opening keys reach the source so
  `cyberlinks` visibility resolves — carried in the session context.

## see also

- cybergraph/specs/query.md — canonical relation schemas (the source of truth)
- [language](language.md) — value model, rules, mutation
- [proof](proof.md) — how a read or an appended set certifies against a root
