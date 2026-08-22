#!/bin/bash

# SpaceKit ASTRA Fact Sheet - PDF Compilation Script
# Requires: pandoc, texlive (for xelatex)

# Colors for output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}================================================${NC}"
echo -e "${BLUE}  SpaceKit ASTRA Fact Sheet - PDF Compilation${NC}"
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

INPUT_FILE="SpaceKit-ASTRA-Fact-Sheet.md"
OUTPUT_FILE="SpaceKit-ASTRA-Fact-Sheet-v1.0.pdf"

echo -e "${GREEN}📄 Compiling: ${INPUT_FILE}${NC}"
echo -e "${GREEN}📦 Output: ${OUTPUT_FILE}${NC}"
echo ""

# Create a header file for formatting and title page
cat > /tmp/header.tex << 'EOF'
\usepackage{fvextra}
\DefineVerbatimEnvironment{Highlighting}{Verbatim}{breaklines,commandchars=\\\{\},fontsize=\small}
\usepackage{fancyvrb}
\RecustomVerbatimEnvironment{verbatim}{Verbatim}{breaklines,breakanywhere,fontsize=\small}
EOF

# Create a custom title page
cat > /tmp/before-body.tex << 'EOF'
\begin{titlepage}
\centering
\vspace*{2cm}
{\Huge\bfseries ASTRA Token\par}
\vspace{0.5cm}
{\Large Investor Fact Sheet\par}
\vspace{2cm}
{\large\itshape SpaceKit — The Quantum-Safe Operating System for Decentralized Agents\par}
\vspace{3cm}
{\large Version 1.0\par}
\vspace{0.5cm}
{\large November 2025\par}
\vspace{3cm}
{\normalsize\textbf{© 2025 SWTCH Labs LLC. All Rights Reserved.}\par}
\vspace{0.3cm}
{\small SpaceKit™ is a product of SWTCH Labs LLC\par}
\vfill
{\small hello@spacekit.xyz | https://spacekit.xyz\par}
\end{titlepage}
\newpage
EOF

# Pandoc compilation with optimal settings for professional PDF
pandoc "${INPUT_FILE}" \
  -f markdown \
  -t pdf \
  --pdf-engine=xelatex \
  --toc \
  --toc-depth=2 \
  -V geometry:margin=1in \
  -V fontsize=11pt \
  -V documentclass=article \
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
  -B /tmp/before-body.tex \
  -o "${OUTPUT_FILE}"

# Capture pandoc exit code before cleanup
PANDOC_EXIT=$?

# Clean up temp files
rm -f /tmp/header.tex /tmp/before-body.tex

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

