---
tags: cyber, soft3, inf
crystal-type: process
crystal-domain: cyber
status: draft
date: 2026-06-07
---
# inf query modes

NOTE (2026-06-07): these data-source modes are now recast under the Trident
register model (pure/static · reactive · live) — see .claude/plans/
inf-spec-plan.md and the eventual specs/extensions.md. mapping: bbg = the
static register's raw committed relations; cybergraph = derived static
relations + reactive `subscribe` + content (the "control over what/how added"
layer); direct = bootstrap dev harness, not a language register. the privacy
resolution below stands unchanged.

inf binds to a data source at session open. the engine (parser, stratify,
magic-set, RA, semi-naive eval, fixed rules) is identical across modes — only
where base relations resolve changes. three modes, increasing abstraction:

```
                 base relations resolve from        provable   writes     content
direct   ─────►  inf's own local store (mem)         no         temp only  no
bbg      ─────►  bbg committed polynomial state      yes        signal     no
cybergraph ───►  cybergraph API (bbg+tru+sync)       yes        signal     yes
```

direct < bbg < cybergraph is a containment order, not three parallel things.
cybergraph composes bbg; bbg is self-contained; direct touches neither.

## the unifying abstraction

one trait, three implementations. inf's RA layer resolves every base relation
through it instead of cozo's direct StoreTx range scans.

```rust
trait RelationSource {
    fn schema(&self, name: &str) -> Option<RelationSchema>;
    fn get(&self, name: &str, key: &Tuple) -> Result<Option<Tuple>>;
    fn scan(&self, name: &str, range: NamespaceRange) -> TupleIter;
    fn snapshot(&self) -> Option<Height>;   // None = local, no chain time
    fn provable(&self) -> bool;             // can reads become Lens openings
    fn writable(&self, name: &str) -> bool; // canonical = false
}
```

a query-local temp store is ALWAYS present regardless of mode — it backs temp
relations and intermediate derivations. a session is therefore: (always-on
temp store) + (one mode-selected RelationSource for canonical relations).

inf never touches fjall in any mode. bbg/cybergraph go through bbg's typed API,
never its disk.

## temp / intermediate store

three concerns must not be conflated:

- STATE ownership — authenticated, committed, persistent. bbg owns this. inf
  owns no state.
- MEMORY allocation — pinned, zero-copy across CPU/GPU/AMX/ANE. honeycrisp
  `unimem` owns this. inf rents from it like every other stack component.
- query-engine DATA STRUCTURES — ordered tuple KV + MVCC + memcmp key
  encoding, driven every fixpoint round by semi-naive eval. this is inf's own
  logic (ported from cozo). nobody else in the stack has it; bbg's persistent
  stores (fjall/redb) and typed ShardStore are the wrong shape for query
  scratch.

the temp store is therefore: inf's ordered-KV/MVCC structure, allocated from
honeycrisp unimem. NOT std-heap. NOT bbg's typed EPHEMERAL dimension — its
get/put by [u8;32] cannot serve ordered range scans over variable tuple keys.

bbg's EPHEMERAL ShardStore and inf's temp store are SIBLINGS: both rent from
the same honeycrisp unimem allocator, they do not nest. evy state → bbg
EPHEMERAL → unimem; inf scratch → inf temp store → unimem. same IOSurface
memory, different structures on top. this is "use the infra we built" done
right — share the allocator, not the abstraction.

one substrate, not two: general intermediates and graph-algo working sets
(pagerank rank vectors, dijkstra frontiers, edge lists) are the SAME
unimem-backed store. everything inf computes lives in unimem, zero-copy to the
GPU/AMX that run the algorithms — no copy boundary between a temp relation and
a kernel's input.

constraints:
- allocation shape: many small tuple inserts per fixpoint round. one IOSurface
  block per tuple is ~1μs each — fatal. use a unimem arena/pool (one block,
  bump/slot allocation, reserve_pool-style) under the ordered map.
- portability: unimem is Apple-Silicon (IOSurface). non-Apple nodes
  (Linux/x86 validators, nation→planet tiers) use a plain heap arena. the temp
  store is generic over a pluggable arena backend — unimem on Apple, heap
  elsewhere — mirroring bbg's memory-vs-unimem ShardStore backends. makes
  honeycrisp a dependency of inf (already true for bbg/trisha).

## the three modes

### direct
inf as an embedded datalog engine. no chain, no proofs. relations live in the
local mem store; read-write. uses: tests, agent-local scratch reasoning,
off-chain analysis over imported data (csv/json fixed rules), prototyping
queries before pointing them at the chain. this is "cozo as-is," carved down.

### bbg
inf over bbg's committed polynomial state. base relations are the raw bbg
evaluation dimensions exposed as relations:

```
particles   (cid => energy, focus φ*)          dimension 0
axons_out   (from => axon ptr)                  dimension 1
axons_in    (to => axon ptr)                    dimension 2
neurons     (id => focus, karma, stake)         dimension 3
coins, cards, files, locations, time, signals   dimensions 4..9
```

read-only over canonical state; snapshot = a block height. every read can
become a Lens opening, so queries here are provable (→ specs/circuit.md). this
is the light-client / validator path: lowest trust assumption, raw aggregates,
verifiable answers. focus and karma ARE here — bbg commits φ* per particle and
focus/karma/stake per neuron.

### cybergraph
inf over cybergraph's relational API. cybergraph is the semantic funnel that
fans out to bbg (store), tru (focus/karma freshness), sync, and radio
(content). it adds over bbg mode:

- derived/normalized relations: `axons` as a clean (from,to => weight,count)
  view; `signals` normalized; the `cyberlinks` view (privacy caveat below).
- tru read-through for values between commitment cycles.
- content reach: queries that need particle CONTENT (not just state) resolve
  through cybergraph → radio NetworkStore; bbg mode cannot.
- friendlier schema (named, typed columns per cybergraph/specs/query.md).

provable for the bbg-backed parts; tru/content parts carry tru's own
commitment or are interactive-only.

## orthogonal axis: interactive vs provable

provability is NOT a fourth mode. it is a per-query attribute available in bbg
and cybergraph modes. a query is provable when: mode is bbg/cybergraph AND all
base relations it reads are provable-source AND every operator has a circuit
encoding AND it uses no banned transcendentals. interactive runs skip proof
emission; the same parsed query produces the same answer either way (proof is
opt-in cost). direct mode is never provable.

ephemeral relations that are query-local derivations of committed data remain
provable as intermediate computation. ephemeral relations populated from
outside (csv import) taint a query as unprovable.

## decided (2026-06-07)

- mode binding is SESSION-LEVEL. temp store always available. no mixing
  bbg+cybergraph in one query — cybergraph already supersets bbg.

- temp store = inf's ordered-KV/MVCC structure (ported from cozo) allocated
  from honeycrisp unimem (arena/pool); heap arena off-Apple. NOT std-heap, NOT
  bbg typed EPHEMERAL. general intermediates and graph-algo buffers are the
  SAME unimem-backed substrate — zero-copy to GPU/AMX/ANE. inf owns the
  structure, honeycrisp owns the memory, bbg owns the state. (see temp/
  intermediate store.)

- direct mode = inf-local embedded engine (no chain). lowest priority.

- cyberlinks privacy: the scopes COMPOSE, they do not exclude each other.
  - `axons` — public aggregate. anyone scans it (bbg + cybergraph mode).
  - `cyberlinks` — neuron-scoped, row-level visibility. a scan returns only
    rows the caller is authorized to see: their OWN links (plaintext, personal
    owner store) ∪ links they hold an opening key for. a bare public scan of
    `cyberlinks` returns nothing.
  `axons` answers "what does the graph say"; `cyberlinks` answers "what did I
  (or what I'm keyed into) assert."

- schema: bbg and cybergraph share ONE clean named/typed schema for relations
  they have in common. the difference is not raw-vs-friendly encoding — it is
  WHICH relations exist and what is reachable. cybergraph is a strict superset:
  adds derived relations (normalized `axons`), tru freshness (focus/karma
  between commitment cycles), and particle CONTENT (via radio). bbg mode
  decodes field elements into the same typed tuples; it just exposes fewer
  relations and no content.

- writes: inf is read-write. both rune (imperative) and inf (declarative mass
  update) mutate the cybergraph through cybergraph → bbg as staked signals.
  canonical mutation is `:link`/`:unlink` emitting a signal; `:put`/`:rm` stay
  for temp relations. writes route through cybergraph regardless of read mode.

## open questions

1. mode discovery: does a query declare its required mode, or does the host
   pick the source and a query fails if it needs more (e.g. asks for content
   in bbg mode)? recommend host-selected source; clear capability-miss error.

2. owner-store key model for `cyberlinks` row visibility — how the caller's
   keys/identity reach the RelationSource at query time (session context).
