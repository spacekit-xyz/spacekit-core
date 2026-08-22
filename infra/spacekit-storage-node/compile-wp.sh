#!/bin/bash

# SpaceKit Storage Node Whitepaper - PDF Compilation Script
# Requires: pandoc, texlive (for xelatex)

# Colors for output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}================================================${NC}"
echo -e "${BLUE}  SpaceKit Network - PDF Compilation${NC}"
echo -e "${BLUE}================================================${NC}"
echo ""

# Check if pandoc is installed
if ! command -v pandoc &> /dev/null; then
    echo "❌ Error: pandoc is not installed"
    echo "Install with: brew install pandoc (macOS) or apt-get install pandoc (Linux)"
    exit 1
fi

# Check if xelatex is installed
if ! command -v xelatex &> /dev/null; then
    echo "⚠️  Warning: xelatex not found. Installing BasicTeX is recommended."
    echo "Install with: brew install --cask basictex (macOS)"
    echo ""
    echo "Attempting compilation anyway..."
fi

INPUT_FILE="./documentation/whitepaper/whitepaper.md"
OUTPUT_FILE="SpaceKit-Storage-Node-Whitepaper-v1.0.pdf"

echo -e "${GREEN}📄 Compiling: ${INPUT_FILE}${NC}"
echo -e "${GREEN}📦 Output: ${OUTPUT_FILE}${NC}"
echo ""

# Create a header file for code block formatting
cat > /tmp/header.tex << 'EOF'
\usepackage{fvextra}
\DefineVerbatimEnvironment{Highlighting}{Verbatim}{breaklines,commandchars=\\\{\},fontsize=\small}
\usepackage{fancyvrb}
\RecustomVerbatimEnvironment{verbatim}{Verbatim}{breaklines,breakanywhere,fontsize=\small}
EOF

# Pandoc compilation with optimal settings for professional PDF
pandoc "${INPUT_FILE}" \
  -f markdown \
  -t pdf \
  --pdf-engine=xelatex \
  --toc \
  --toc-depth=3 \
  -V geometry:margin=1in \
  -V fontsize=11pt \
  -V documentclass=article \
  -V classoption=titlepage \
  -V titlepage=true \
  -V papersize=letter \
  -V colorlinks=true \
  -V linkcolor=blue \
  -V urlcolor=blue \
  -V toccolor=black \
  --syntax-highlighting=tango \
  --number-sections=false \
  -V mainfont="Times New Roman" \
  -V monofont="Courier New" \
  -V monofontoptions="Scale=0.75" \
  -H /tmp/header.tex \
  -o "${OUTPUT_FILE}"

# Capture pandoc exit code before cleanup
PANDOC_EXIT=$?

# Clean up temp file
rm -f /tmp/header.tex

# Check if compilation was successful
if [ $PANDOC_EXIT -eq 0 ] && [ -f "${OUTPUT_FILE}" ]; then
    echo ""
    echo -e "${GREEN}✅ Success! PDF generated: ${OUTPUT_FILE}${NC}"
    echo ""
    
    # Get file size
    SIZE=$(du -h "${OUTPUT_FILE}" | cut -f1)
    PAGES=$(pdfinfo "${OUTPUT_FILE}" 2>/dev/null | grep "Pages:" | awk '{print $2}')
    
    if [ -n "$PAGES" ]; then
        echo "📊 Document Stats:"
        echo "   • Pages: ${PAGES}"
        echo "   • Size: ${SIZE}"
    fi
    
    echo ""
    echo "🚀 Opening PDF..."
    open "${OUTPUT_FILE}" 2>/dev/null || xdg-open "${OUTPUT_FILE}" 2>/dev/null || echo "Please open ${OUTPUT_FILE} manually"
else
    echo ""
    echo -e "❌ Compilation failed. Check errors above."
    exit 1
fi

