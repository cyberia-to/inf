---
tags: cyber, soft3, inf
crystal-type: spec
crystal-domain: cyber
---
# inf functions

builtin reference for [[inf]]. functions appear in rule bodies, expressions, and
aggregation. inf is pure relation; it does not implement arithmetic itself —
each family delegates to the sibling language that owns its algebra. this file
maps the families to their owners and marks which belong to the pure (provable)
subset.

## determinism rules

the pure subset is deterministic over a committed snapshot ([[trident]]
constraint 2). three exclusions follow and hold across every family below:

- no floating point. numbers are exact field elements or words. `to_float` and
  float literals have no form in inf.
- no clock. `now` is excluded; time comes from the committed snapshot (block
  height) and the `Validity` time dimension, not a wall clock.
- no unseeded randomness. the `rand_*` family is excluded from the pure subset;
  it exists only in the bootstrap.

## arithmetic — [[Trident|Tri]]

| function | notes |
|---|---|
| `add` `sub` `mul` | field operations |
| `div` | field inverse; division by zero is rejected |
| `neg` `abs` `signum` | field / sign |

pure. operate on field atoms. integer overflow is handled by the field modulus;
widening where a product could exceed the field is Tri's concern.

## word and bitwise — [[Rs]] / [[Bt]]

| function | owner | notes |
|---|---|---|
| `mod` `shl` `shr` | Rs | 32-bit word ops |
| `and` `or` `xor` `not` (bitwise) | Bt | bit-tower ops on word/tag atoms |
| `floor` `ceil` `round` | Rs | over fixed-point words |

pure.

## comparison and boolean — [[Trident|Tri]] / [[Bt]]

| function | notes |
|---|---|
| `eq` `neq` `lt` `le` `gt` `ge` | field comparison (Tri) |
| `and` `or` `not` (logical) | boolean (Bt), 0/1 field |
| `max` `min` | extrema |

pure.

## bytes and text — [[Rs]]

strings are particle content. text functions decode the byte content of a
particle and are deterministic over it.

| function | notes |
|---|---|
| `length` | byte / char count |
| `concat` `slice` | byte ops |
| `includes` `starts_with` `ends_with` | substring tests |
| `lowercase` `uppercase` `trim` | case / whitespace |

pure over committed content. `regex_*` (match, replace, extract) also lower to
Rs; they are pure but carry higher cost (see [cost](cost.md)).

## structure and lists — [[nox]]

lists are nox cons-structure; list functions are structural, not arithmetic.

| function | notes |
|---|---|
| `list` `first` `last` `get` | construct / access |
| `slice` `concat` `reverse` `sorted` | sequence ops |
| `length` `is_in` | size / membership |
| `union` `intersection` `difference` | set ops on lists |

pure.

## vector — [[Ten]]

embedding-based comparison over particle vectors.

| function | notes |
|---|---|
| `vec` | construct a vector |
| `l2_normalize` `l2_dist` `ip_dist` `cos_dist` | distances |

pure. lower to Ten. used by similarity queries and the vector fixed rules.

## json — [[Rs]]

json is byte content; access is deterministic over it.

| function | notes |
|---|---|
| `parse_json` `dump_json` | decode / encode |
| `get` `set_json_path` `remove_json_path` | access / edit a value |

pure over content.

## type and identity

| function | notes |
|---|---|
| `to_int` `to_bytes` `to_string` | conversion (no `to_float`) |
| `is_int` `is_bytes` `is_string` `is_list` `is_null` | type tests |
| `coalesce` | first non-null |

pure.

## time

| function | notes |
|---|---|
| `validity` | construct a `Validity` for a time-travel read |
| `format_timestamp` `parse_timestamp` | Rs byte ops over a committed timestamp |

`now` is excluded from the pure subset (clock). a query reads time from the
snapshot height or a `Validity` column, both committed.

## excluded from the pure subset

available only in the bootstrap, never in a provable query:

- transcendentals — `sqrt` `exp` `ln` `log2` `sin` `cos` `tan` and the
  hyperbolic / inverse families. no field-exact form. a future fixed-point
  approximation with documented error bounds may admit some via Tri.
- `rand_float` `rand_int` `rand_bernoulli` `rand_uuid_*` `rand_vec` — randomness.
- `now` — clock.

## aggregation operators

applied in rule heads; variables without an operator are grouping keys.

| operator | semi-lattice | notes |
|---|---|---|
| `count` `sum` | no | over field/word |
| `min` `max` | yes | admissible in recursion |
| `mean` | no | kept as `(sum, count)`; the quotient is a field `div` (Tri) |
| `collect` `unique` | no / yes | gather into a list |
| `union` `intersection` `choice` | yes | set / pick |

semi-lattice operators converge monotonically and are admissible inside
recursive rules. `mean` carries `(sum, count)` rather than a running float; the
division is exact field arithmetic, requested only when the quotient is read.

## see also

- [language](language.md) — value model and rules
- [algorithms](algorithms.md) — fixed-rule graph algorithms
- [interop](interop.md) — the calling boundary to Tri/Rs/Bt/Ten
