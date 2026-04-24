# Shared scaffolding detector

Finds groups of classes in the same directory subtree that share the same
**overall method-shape** (same method names, visibilities, and order) and
have high **per-method body similarity**. These groups are candidates for
extraction into a shared base class.

## What it catches

Existing detectors (`duplicate_function`, `near_duplicate`,
`parallel_implementation`) compare pairs of functions. They miss class-level
scaffolding — where dozens of classes each follow the same skeleton with
small variations.

For example, a plugin with ~90 ability classes all following
`__construct → registerAbility → execute → checkPermission` surfaces as one
`shared_scaffolding` finding, not 90 unrelated parallel-implementation pairs.

## Algorithm

1. For each fingerprinted file that declares a class, build an ordered
   shape signature of `(method_name, visibility)` tuples using
   `FileFingerprint.methods` and `FileFingerprint.visibility`.
2. Bucket classes by `(subtree_root, shape_signature)`. The subtree root is
   the first two path components under the component root (e.g. files under
   `inc/Abilities/**` share the `inc/Abilities` bucket).
3. For each bucket with ≥ 3 classes, compute mean per-method body similarity
   using `FileFingerprint.method_hashes`. For each method in the shape, the
   similarity is the size of the largest identical-hash bucket divided by
   the member count.
4. If the mean similarity is ≥ 60%, emit one finding (severity `warning`,
   kind `shared_scaffolding`) describing the group.

## Finding fields

- `file`: the subtree root (e.g. `inc/Abilities`).
- `description`: member count, shape, mean body similarity, identical
  method-body count, member class list, and an estimated LOC reduction.
- `suggestion`: proposes extracting a shared base class under the subtree.

## Thresholds

| Parameter              | Value | Rationale                                   |
| ---------------------- | ----- | ------------------------------------------- |
| Minimum group size     | 3     | Pairs are too noisy; 3+ is a real pattern.  |
| Minimum mean body sim. | 0.60  | Below this, classes diverge too much to     |
|                        |       | share meaningful scaffolding.               |
| Subtree depth          | 2     | Groups stay within logical module roots.    |

## Related detectors

- `duplicate_function` / `near_duplicate` — function-level body similarity.
- `parallel_implementation` — similar call patterns across function pairs.
- `shadow_module` — whole directories that are near-copies.
