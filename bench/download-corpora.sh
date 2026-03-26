#!/usr/bin/env bash
set -euo pipefail

# Download real-world XML corpora for benchmarking.
# Following the icXML approach: varied markup densities and structures.

DIR="$(cd "$(dirname "$0")" && pwd)/corpora"
mkdir -p "$DIR"

echo "Downloading benchmark corpora to $DIR/"
echo ""

# ================================================================
# 1. DBLP — 4GB, computer science bibliography
#    Medium markup density, 7M+ records, single huge file
# ================================================================
if [ ! -f "$DIR/dblp.xml" ]; then
    echo ">>> DBLP (~400MB compressed, ~4GB uncompressed)"
    curl -L -o "$DIR/dblp.xml.gz" "https://dblp.org/xml/dblp.xml.gz"
    # Also grab the DTD (required for entity resolution)
    curl -L -o "$DIR/dblp.dtd" "https://dblp.org/xml/dblp.dtd"
    echo "  Decompressing..."
    gunzip "$DIR/dblp.xml.gz"
    echo "  Done: $(ls -lh "$DIR/dblp.xml" | awk '{print $5}')"
else
    echo ">>> DBLP: already exists ($(ls -lh "$DIR/dblp.xml" | awk '{print $5}'))"
fi
echo ""

# ================================================================
# 2. OSM Liechtenstein — ~50MB, attribute-heavy geographic data
#    Low text density, high attribute density (lat/lon/id on every node)
# ================================================================
if [ ! -f "$DIR/liechtenstein.osm" ]; then
    echo ">>> OSM Liechtenstein (~10MB compressed, ~50MB uncompressed)"
    curl -L -o "$DIR/liechtenstein.osm.bz2" \
        "https://download.geofabrik.de/europe/liechtenstein-latest.osm.bz2"
    echo "  Decompressing..."
    bunzip2 "$DIR/liechtenstein.osm.bz2"
    echo "  Done: $(ls -lh "$DIR/liechtenstein.osm" | awk '{print $5}')"
else
    echo ">>> OSM: already exists ($(ls -lh "$DIR/liechtenstein.osm" | awk '{print $5}'))"
fi
echo ""

# ================================================================
# 3. PubMed — 5 baseline files, ~10-20MB each compressed
#    Medium-high markup density, deep structure, real medical XML
# ================================================================
PUBMED_COUNT=0
for i in 0001 0002 0003 0004 0005; do
    f="pubmed25n${i}.xml"
    if [ -f "$DIR/$f" ]; then
        PUBMED_COUNT=$((PUBMED_COUNT + 1))
        continue
    fi
    echo ">>> PubMed baseline $i"
    curl -L -o "$DIR/${f}.gz" \
        "https://ftp.ncbi.nlm.nih.gov/pubmed/baseline/pubmed25n${i}.xml.gz"
    echo "  Decompressing..."
    gunzip "$DIR/${f}.gz"
    PUBMED_COUNT=$((PUBMED_COUNT + 1))
    echo "  Done: $(ls -lh "$DIR/$f" | awk '{print $5}')"
done
if [ $PUBMED_COUNT -eq 5 ]; then
    echo ">>> PubMed: 5 files ready"
fi
echo ""

# ================================================================
# 4. XMark — synthetic benchmark, standard academic reference
#    Needs xmlgen compiled from source
# ================================================================
if [ ! -f "$DIR/xmark-116mb.xml" ]; then
    echo ">>> XMark (building xmlgen, then generating 116MB + 1.16GB)"
    XMARK_DIR="$DIR/.xmlgen-src"
    if [ ! -f "$XMARK_DIR/xmlgen" ]; then
        mkdir -p "$XMARK_DIR"
        curl -L -o "$XMARK_DIR/xmlgen.tar.gz" \
            "https://projects.cwi.nl/xmark/Assets/xmlgen.tar.gz" 2>/dev/null || \
        curl -L -o "$XMARK_DIR/xmlgen.tar.gz" \
            "https://raw.githubusercontent.com/eliben/xmlgen/main/xmlgen.tar.gz" 2>/dev/null || true
        if [ -f "$XMARK_DIR/xmlgen.tar.gz" ]; then
            cd "$XMARK_DIR"
            tar xzf xmlgen.tar.gz 2>/dev/null || true
            if [ -f "xmlgen.c" ]; then
                cc -O2 -o xmlgen xmlgen.c 2>/dev/null || echo "  xmlgen compilation failed"
            fi
            cd -
        fi
    fi
    if [ -f "$XMARK_DIR/xmlgen" ]; then
        echo "  Generating XMark factor 1 (116MB)..."
        "$XMARK_DIR/xmlgen" -f 1 -o "$DIR/xmark-116mb.xml"
        echo "  Done: $(ls -lh "$DIR/xmark-116mb.xml" | awk '{print $5}')"
    else
        echo "  WARNING: Could not build xmlgen. Skipping XMark."
        echo "  Try: git clone https://github.com/eliben/xmlgen && cd xmlgen && make"
    fi
else
    echo ">>> XMark: already exists ($(ls -lh "$DIR/xmark-116mb.xml" | awk '{print $5}'))"
fi
echo ""

# ================================================================
# Summary
# ================================================================
echo "=== Corpus Summary ==="
for f in "$DIR"/*.xml "$DIR"/*.osm; do
    [ -f "$f" ] && echo "  $(basename "$f"): $(ls -lh "$f" | awk '{print $5}')"
done
echo ""
echo "Total: $(du -sh "$DIR" | cut -f1)"
