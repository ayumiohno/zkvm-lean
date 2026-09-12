# Demo assets

Used by `scripts/demo.sh`.

| | |
| --- | --- |
| `public.bin` / `public.ndjson` | the public environment (`Preimage.f`'s closure) |
| `full.bin` / `full.ndjson` | public environment + proof term |
| `expected.ndjson` | **the proposition a verifier wrote themselves** (proof `sorry`) |
| `manifest.json` | the official image IDs |
| `other.ndjson` / `other_public.bin` | a different proposition and environment (negative tests) |
| `receipt.bin` | **must be generated** (below) |
| `tampered.bin` | derived automatically from `receipt.bin` |

## Generating `receipt.bin`

26.5M cycles: **71 s on a Quadro RTX 8000**, about an hour on CPU.

```sh
compose demo/public.bin demo/full.bin Preimage.knows_preimage \
        --assume-prelude --prove --out demo/receipt.bin
```

**A receipt is invalidated when the image ID changes.** After rebuilding the guest,
regenerate both `manifest.json` and the receipt. Use `scripts/build-release.sh`
(Docker) for anything published.
