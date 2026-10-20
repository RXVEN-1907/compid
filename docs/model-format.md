# The model format: compid-model/v1

`model/model.json` is a single static JSON file: the Python trainer writes
it, the Rust CLI reads it, and this document is the contract between them.
The format string inside the file versions the layout, and
`feature_version` versions the feature schema the model was trained against
(currently `"1"`, as specified in `docs/dataset.md`).

The per-function model `model/functions.json` uses this exact format too -
same `format`/`feature_version`/`created`/`features`/`fields` layout, same
reader validation in `src/model.rs`, no version bump. Two things differ,
both data rather than format: the `features` list draws from the
function-scoped schema (`ng:`, `mn:`, `rg:`, `cn:`, `fn:<log2size>`,
`meta:windows` - no `sec:`, `ent:`, or `cmt:`), and `fields` holds only the
fields a single function witnesses (`compiler_family`, `opt_level`). The
whole-binary CLI would happily load `functions.json`; the per-function CLI
defaults to it. `docs/FUNCTIONS-EVALUATION.md` reports its hold-out
numbers.

## Example

```json
{
  "format": "compid-model/v1",
  "feature_version": "1",
  "created": "2026-10-20",
  "features": [
    "mn:mov",
    "mn:push",
    "rg:rax",
    "meta:windows",
    "cmt:clang"
  ],
  "fields": {
    "compiler_family": {
      "classes": ["gcc", "clang"],
      "intercept": [0.12, -0.12],
      "coef": [
        [-0.42, 0.31, -0.05, -0.18, -1.02],
        [0.38, -0.27, 0.04, 0.21, 1.10]
      ]
    },
    "pie": {
      "classes": ["no", "yes"],
      "intercept": [0.03, -0.03],
      "coef": [
        [0.11, -0.06, 0.02, -0.09, 0.0],
        [-0.08, 0.05, -0.01, 0.10, 0.0]
      ]
    }
  }
}
```

## Fields

| key               | type             | meaning                                                                      |
|-------------------|------------------|------------------------------------------------------------------------------|
| `format`          | string, literal  | always `compid-model/v1`; the reader rejects anything else                    |
| `feature_version` | string           | the feature schema the model was trained on (`"1"` for the current schema)    |
| `created`         | ISO date string  | the date of the training run that produced the file                           |
| `features`        | array of strings | ordered feature names; positions here are referenced by every coef vector     |
| `fields`          | object           | one entry per label field the model predicts (any subset of the nine fields)  |

Each entry under `fields` is keyed by the label field name and holds:

| key         | type                    | dimension rule                                  |
|-------------|-------------------------|-------------------------------------------------|
| `classes`   | array of strings        | class names, in output order                    |
| `intercept` | array of floats         | one per class, same order as `classes`          |
| `coef`      | array of float arrays   | one row per class; each row has exactly one float per entry of `features` |

If `features` has length `F` and a field has `C` classes, its `intercept`
has length `C` and its `coef` is `C` by `F`. Class order is meaningful: index
`c` in `classes` corresponds to `intercept[c]` and `coef[c]`.

## Inference algorithm

The Rust reader implements exactly this, nothing more:

1. Extract the binary's feature JSON (`compid extract`).
2. Build the tf vector `v` over the model's `features` list: for each named
   feature take the count from the extractor output, or `0` if the extractor
   did not emit it. Values the extractor emits but the model does not list
   are ignored.
3. L2-normalize: `x = v / ||v||₂`. (An all-zero vector stays all-zero.)
4. For each field, for each class `c`:
   `score(c) = intercept(c) + Σᵢ coef(c)(i) · x(i)`.
5. Per field, `probabilities = softmax(scores)`.
6. Report the top-3 classes by probability, with their probabilities.

Fields present in `fields` are reported; fields absent from the model are not
predicted. `compid classify` prints these as a human table or, with `--json`,
as an object.

## Reader validation

The Rust reader rejects a model file that fails any of:

- `format` is not `"compid-model/v1"`;
- `feature_version` is not a known feature version (`"1"` today);
- `features` is missing, empty, or contains duplicates;
- any field's `intercept` length differs from its `classes` length;
- any field's `coef` is not exactly `classes.length` rows of
  `features.length` floats;
- any JSON type mismatch (no silent coercions).

A model that passes validation but was trained on a different
`feature_version` than the extractor's current output is still loadable;
mismatches between the two versions are flagged, not guessed around.

## Versioning rules

- Changing the feature schema (`docs/dataset.md`) bumps `feature_version` and
  requires a retrain plus a refresh of `dataset/features/` and
  `model/model.json` in the same change.
- Changing this layout bumps the format string (the next one would be
  `compid-model/v2`) and must update this document, the Python writer, and
  the Rust reader in the same change.
- Adding rows to the dataset changes none of the above - data is not format.
  Retrain and commit the refreshed model.
