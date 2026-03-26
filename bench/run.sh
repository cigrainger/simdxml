#!/usr/bin/env bash
set -euo pipefail

# Benchmark sxq against other XML/XPath CLI tools.
#
# All benchmarks pipe output to `wc -c` to force tools to actually produce
# output while keeping terminal rendering out of the measurement.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

SXQ="$ROOT/target/release/sxq"
PUGIXML="$SCRIPT_DIR/pugixml-xpath"

cargo build --release -p simdxml-cli --manifest-path="$ROOT/Cargo.toml" 2>/dev/null

PATENT_US="$ROOT/testdata/realworld/patent-us.xml"
HUGE="$ROOT/testdata/realworld/huge.xml"
GIGANTIC="$ROOT/testdata/realworld/gigantic.svg"
ATTR_HEAVY="$ROOT/testdata/bench/attrheavy_xlarge.xml"

W=5   # warmup
R=30  # runs
EXPORT_DIR="$SCRIPT_DIR/results"
mkdir -p "$EXPORT_DIR"

echo "============================================"
echo "  sxq benchmark suite"
echo "============================================"
echo ""
echo "Files:"
for f in "$PATENT_US" "$HUGE" "$GIGANTIC" "$ATTR_HEAVY"; do
    sz=$(wc -c < "$f" | tr -d ' ')
    echo "  $(basename "$f"): $(( sz / 1024 )) KB"
done
echo ""
echo "Output piped to wc -c (forces output, avoids terminal overhead)."
echo ""

# ================================================================
# 1. Small file, simple query (86 KB patent)
# ================================================================
echo ">>> 1. //invention-title — patent-us.xml (86 KB)"
echo ""

hyperfine --warmup $W --runs $R -i \
    --export-json "$EXPORT_DIR/01_small_simple.json" \
    -n "sxq"        "$SXQ '//invention-title' $PATENT_US | wc -c" \
    -n "pugixml"    "$PUGIXML '//invention-title' $PATENT_US | wc -c" \
    -n "xmllint"    "xmllint --xpath '//invention-title/text()' $PATENT_US | wc -c" \
    -n "xmlstarlet" "xmlstarlet sel -t -v '//invention-title' $PATENT_US | wc -c" \
    -n "xq"         "xq -x '//invention-title' $PATENT_US | wc -c" \
    -n "xidel"      "xidel $PATENT_US -e '//invention-title' --silent | wc -c" \
    -n "xee"        "xee xpath '//invention-title' $PATENT_US | wc -c"

echo ""

# ================================================================
# 2. Small file, many matches (86 KB patent, 31 claim-text hits)
# ================================================================
echo ">>> 2. //claim-text — patent-us.xml (86 KB, 31 hits)"
echo ""

hyperfine --warmup $W --runs $R -i \
    --export-json "$EXPORT_DIR/02_small_many.json" \
    -n "sxq"        "$SXQ '//claim-text' $PATENT_US | wc -c" \
    -n "pugixml"    "$PUGIXML '//claim-text' $PATENT_US | wc -c" \
    -n "xmllint"    "xmllint --xpath '//claim-text' $PATENT_US | wc -c" \
    -n "xmlstarlet" "xmlstarlet sel -t -v '//claim-text' $PATENT_US | wc -c" \
    -n "xq"         "xq -x '//claim-text' $PATENT_US | wc -c" \
    -n "xidel"      "xidel $PATENT_US -e '//claim-text' --silent | wc -c" \
    -n "xee"        "xee xpath '//claim-text' $PATENT_US | wc -c"

echo ""

# ================================================================
# 3. Large SVG (1.3 MB, 80 path elements — self-closing, no text)
# ================================================================
echo ">>> 3. //path — gigantic.svg (1.3 MB, 80 hits)"
echo "   Note: path elements are self-closing (attribute-heavy, no text content)"
echo ""

hyperfine --warmup $W --runs $R -i \
    --export-json "$EXPORT_DIR/03_large_svg.json" \
    -n "sxq -r"     "$SXQ -r '//path' $GIGANTIC | wc -c" \
    -n "pugixml"    "$PUGIXML '//path' $GIGANTIC | wc -c" \
    -n "xmllint"    "xmllint --xpath '//path' $GIGANTIC | wc -c" \
    -n "xmlstarlet" "xmlstarlet sel -t -c '//path' $GIGANTIC | wc -c" \
    -n "xq"         "xq -x '//path' $GIGANTIC | wc -c" \
    -n "xidel"      "xidel $GIGANTIC -e '//path' --silent | wc -c" \
    -n "xee"        "xee xpath '//path' $GIGANTIC | wc -c"

echo ""

# ================================================================
# 4. Huge namespaced XML (835 KB, 425 keyword elements)
# xmllint (silent 0 results), xmlstarlet (error), xee (error)
# cannot handle namespace prefixes without registration.
# ================================================================
echo ">>> 4. //gmd:keyword — huge.xml (835 KB, 425 hits)"
echo "   xmllint/xmlstarlet/xee excluded: fail on namespace prefixes."
echo ""

hyperfine --warmup $W --runs $R -i \
    --export-json "$EXPORT_DIR/04_huge_ns.json" \
    -n "sxq"     "$SXQ '//gmd:keyword' $HUGE | wc -c" \
    -n "pugixml" "$PUGIXML '//gmd:keyword' $HUGE | wc -c" \
    -n "xq"      "xq -x '//gmd:keyword' $HUGE | wc -c" \
    -n "xidel"   "xidel $HUGE -e '//gmd:keyword' --silent | wc -c"

echo ""

# ================================================================
# 5. Attribute-heavy XML (10 MB, 70K self-closing records)
# The SIMD sweet spot: lots of quoted attribute values to scan.
# Uses -r/raw mode since elements have no text content.
# ================================================================
ATTR_KB=$(( $(wc -c < "$ATTR_HEAVY" | tr -d ' ') / 1024 ))
echo ">>> 5. //record — attrheavy_xlarge.xml (${ATTR_KB} KB, 70K hits)"
echo "   Attribute-heavy: self-closing <record id=.. a0=.. a1=.. .../>"
echo ""

hyperfine --warmup $W --runs $R -i \
    --export-json "$EXPORT_DIR/05_attr_heavy.json" \
    -n "sxq -r"     "$SXQ -r '//record' $ATTR_HEAVY | wc -c" \
    -n "pugixml"    "$PUGIXML '//record' $ATTR_HEAVY | wc -c" \
    -n "xmllint"    "xmllint --xpath '//record' $ATTR_HEAVY | wc -c" \
    -n "xmlstarlet" "xmlstarlet sel -t -c '//record' $ATTR_HEAVY | wc -c" \
    -n "xq"         "xq -x '//record' $ATTR_HEAVY | wc -c" \
    -n "xidel"      "xidel $ATTR_HEAVY -e '//record' --silent | wc -c" \
    -n "xee"        "xee xpath '//record' $ATTR_HEAVY | wc -c"

echo ""

# ================================================================
# 6. Scalar expression — count()
# ================================================================
echo ">>> 6. count(//claim) — patent-us.xml (86 KB)"
echo ""

hyperfine --warmup $W --runs $R -i \
    --export-json "$EXPORT_DIR/06_count.json" \
    -n "sxq"     "$SXQ 'count(//claim)' $PATENT_US | wc -c" \
    -n "xmllint" "xmllint --xpath 'count(//claim)' $PATENT_US | wc -c" \
    -n "xidel"   "xidel $PATENT_US -e 'count(//claim)' --silent | wc -c" \
    -n "xee"     "xee xpath 'count(//claim)' $PATENT_US | wc -c"

echo ""
echo "============================================"
echo "  Done. Results in $EXPORT_DIR/"
echo "============================================"
