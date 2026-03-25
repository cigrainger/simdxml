# simdxml

**SIMD-accelerated XML parser with full XPath 1.0 support.**

The first production SIMD XML parser. Uses a two-pass structural indexing architecture (adapted from [simdjson](https://github.com/simdjson/simdjson)) to parse XML at multi-GB/s speeds, then evaluates XPath 1.0 expressions against flat arrays instead of a DOM tree.

## Status

**Phase 1 complete:** Scalar parser + full XPath 1.0 engine with all 13 axes.
Phase 2 (SIMD structural indexer) in progress.

## Quick Start

### As a library

```rust
use simdxml::{parse, CompiledXPath};

let xml = br#"<patent><claim>A device for...</claim></patent>"#;
let index = parse(xml).unwrap();

// One-shot XPath
let texts = index.xpath_text("//claim").unwrap();
assert_eq!(texts, vec!["A device for..."]);

// Compiled XPath (reusable across documents)
let expr = CompiledXPath::compile("//claim").unwrap();
let texts = expr.eval_text(&index).unwrap();
```

### As a CLI

```bash
# Extract patent claims
simdxml query -e '//claim' patent.xml

# Extract title
simdxml query -e '/patent/title' patent.xml

# Show structural index info
simdxml info patent.xml
```

### As a DuckDB extension

```sql
LOAD 'duckdb_xpath';

SELECT * FROM xpath_text('<patent><claim>text</claim></patent>', '//claim');
```

## XPath 1.0 Support

All 13 axes supported:
- `child`, `descendant`, `descendant-or-self`
- `parent`, `ancestor`, `ancestor-or-self`
- `following-sibling`, `preceding-sibling`
- `following`, `preceding`
- `self`, `attribute`, `namespace`

Node tests: `name`, `*`, `text()`, `node()`, `comment()`, `processing-instruction()`

Abbreviated syntax: `/`, `//`, `..`, `.`, `@`

## Architecture

Two-pass structural indexing (no DOM):

1. **Stage 1:** Scan XML bytes, detect structural characters, build flat index arrays
2. **Stage 2:** Evaluate XPath against the index using array operations

The structural index uses ~16 bytes per tag vs ~35 bytes per node in a typical DOM.

## License

MIT OR Apache-2.0
