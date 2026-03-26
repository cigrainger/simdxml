#!/usr/bin/env bash
set -euo pipefail

# sxq benchmark suite.
#
# Measures end-to-end performance: file I/O + parse + XPath eval + output.
# All tools pipe output to wc -c to force real output without terminal overhead.
#
# Requires: hyperfine, xmllint, xmlstarlet, xq, xidel, xee
# Plus: bench/pugixml-xpath (compiled), target/release/sxq (built)

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
SXQ="$ROOT/target/release/sxq"
PUGIXML="$SCRIPT_DIR/pugixml-xpath"

cargo build --release -p simdxml-cli --manifest-path="$ROOT/Cargo.toml" 2>/dev/null

PATENT_US="$ROOT/testdata/realworld/patent-us.xml"
ATTR_HEAVY="$ROOT/testdata/bench/attrheavy_xlarge.xml"
CORPUS="$ROOT/testdata/bench/corpus"
PUBMED="$ROOT/bench/corpora/pubmed26n0001.xml"
DBLP="$ROOT/bench/corpora/dblp.xml"

W=5; R=20
EXPORT="$SCRIPT_DIR/results"
mkdir -p "$EXPORT"

echo "============================================"
echo "  sxq benchmark suite"
echo "============================================"
echo ""

# Show file sizes
for label_file in \
    "patent-us.xml:$PATENT_US" \
    "attrheavy_xlarge.xml:$ATTR_HEAVY" \
    "corpus (500 patents):$CORPUS" \
    "pubmed26n0001.xml:$PUBMED" \
    "dblp.xml:$DBLP"; do
    label="${label_file%%:*}"
    f="${label_file#*:}"
    if [ -d "$f" ]; then
        n=$(ls "$f"/*.xml 2>/dev/null | wc -l | tr -d ' ')
        echo "  $label: $n files"
    elif [ -f "$f" ]; then
        sz=$(ls -lh "$f" | awk '{print $5}')
        echo "  $label: $sz"
    else
        echo "  $label: NOT FOUND"
    fi
done
echo ""

# ================================================================
# 1. PubMed 195MB — the real-world headline benchmark
# ================================================================
if [ -f "$PUBMED" ]; then
    echo ">>> 1. PubMed — //Article (195 MB, 30K articles)"
    echo ""
    hyperfine --warmup $W --runs $R -i \
        --export-json "$EXPORT/01_pubmed.json" \
        -n "sxq"        "$SXQ -c '//Article' $PUBMED" \
        -n "sxq -t1"    "$SXQ -t 1 -c '//Article' $PUBMED" \
        -n "pugixml"    "$PUGIXML '//Article' $PUBMED | wc -c" \
        -n "xmllint"    "xmllint --xpath '//Article' $PUBMED | wc -c" \
        -n "xq"         "xq -x '//Article' $PUBMED | wc -c" \
        -n "xee"        "xee xpath '//Article' $PUBMED | wc -c"
    echo ""
fi

# ================================================================
# 2. DBLP 5.1GB — stress test, largest file
# ================================================================
if [ -f "$DBLP" ]; then
    echo ">>> 2. DBLP — //article (5.1 GB, 4.2M articles)"
    echo ""
    hyperfine --warmup 2 --runs 5 -i \
        --export-json "$EXPORT/02_dblp.json" \
        -n "sxq"        "$SXQ -c '//article' $DBLP" \
        -n "pugixml"    "$PUGIXML '//article' $DBLP | wc -c"
    echo ""
fi

# ================================================================
# 3. Attribute-heavy 10MB — SIMD sweet spot
# ================================================================
echo ">>> 3. Attribute-heavy — //record (10 MB, 70K self-closing records)"
echo ""
hyperfine --warmup $W --runs $R -i \
    --export-json "$EXPORT/03_attr_heavy.json" \
    -n "sxq"        "$SXQ -r '//record' $ATTR_HEAVY | wc -c" \
    -n "pugixml"    "$PUGIXML '//record' $ATTR_HEAVY | wc -c" \
    -n "xmllint"    "xmllint --xpath '//record' $ATTR_HEAVY | wc -c" \
    -n "xmlstarlet" "xmlstarlet sel -t -c '//record' $ATTR_HEAVY | wc -c" \
    -n "xq"         "xq -x '//record' $ATTR_HEAVY | wc -c" \
    -n "xidel"      "xidel $ATTR_HEAVY -e '//record' --silent | wc -c" \
    -n "xee"        "xee xpath '//record' $ATTR_HEAVY | wc -c"
echo ""

# ================================================================
# 4. Patent corpus — 500 files, single invocation
# ================================================================
if [ -d "$CORPUS" ]; then
    echo ">>> 4. Patent corpus — //claim (500 files, 39 MB total)"
    echo ""
    hyperfine --warmup $W --runs $R -i \
        --export-json "$EXPORT/04_corpus.json" \
        -n "sxq"        "$SXQ -c '//claim' $CORPUS/*.xml" \
        -n "pugixml"    "$PUGIXML '//claim' $CORPUS/*.xml | wc -c"
    echo ""
fi

# ================================================================
# 5. Small file — all tools (startup-dominated, for reference)
# ================================================================
echo ">>> 5. Single patent — //claim-text (86 KB, baseline)"
echo ""
hyperfine --warmup $W --runs $R -i \
    --export-json "$EXPORT/05_small.json" \
    -n "sxq"        "$SXQ '//claim-text' $PATENT_US | wc -c" \
    -n "pugixml"    "$PUGIXML '//claim-text' $PATENT_US | wc -c" \
    -n "xmllint"    "xmllint --xpath '//claim-text' $PATENT_US | wc -c" \
    -n "xmlstarlet" "xmlstarlet sel -t -v '//claim-text' $PATENT_US | wc -c" \
    -n "xq"         "xq -x '//claim-text' $PATENT_US | wc -c" \
    -n "xidel"      "xidel $PATENT_US -e '//claim-text' --silent | wc -c" \
    -n "xee"        "xee xpath '//claim-text' $PATENT_US | wc -c"
echo ""

echo "============================================"
echo "  Done. Results in $EXPORT/"
echo "============================================"
