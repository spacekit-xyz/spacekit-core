#!/bin/bash

# SpaceKit Technical Whitepaper - PDF Compilation Script
# Requires: pandoc, texlive (for xelatex)

# Colors for output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}================================================${NC}"
echo -e "${BLUE}  SpaceKit v1.1 - Technical Whitepaper${NC}"
echo -e "${BLUE}================================================${NC}"
echo ""

# Check if pandoc is installed
if ! command -v pandoc &> /dev/null; then
    echo "Error: pandoc is not installed"
    echo "Install with: brew install pandoc (macOS) or apt-get install pandoc (Linux)"
    exit 1
fi

# Check if xelatex is installed
if ! command -v xelatex &> /dev/null; then
    echo "Warning: xelatex not found. Installing BasicTeX is recommended."
    echo "Install with: brew install --cask basictex (macOS)"
    echo ""
    echo "Attempting compilation anyway..."
fi

INPUT_FILE="SpaceKit-Whitepaper.md"
OUTPUT_FILE="SpaceKit-Technical-Whitepaper-v1.1.pdf"

echo -e "${GREEN}Compiling: ${INPUT_FILE}${NC}"
echo -e "${GREEN}Output: ${OUTPUT_FILE}${NC}"
echo ""

# Create a header file for code block formatting and title page
cat > /tmp/header.tex << 'EOF'
\usepackage{fvextra}
\DefineVerbatimEnvironment{Highlighting}{Verbatim}{breaklines,commandchars=\\\{\},fontsize=\small}
\usepackage{fancyvrb}
\RecustomVerbatimEnvironment{verbatim}{Verbatim}{breaklines,breakanywhere,fontsize=\small}

% Title page styling
\usepackage{xcolor}

% Define SpaceKit brand colors
\definecolor{spacekitprimary}{RGB}{30, 64, 175}
\definecolor{spacekitaccent}{RGB}{99, 102, 241}
\definecolor{spacekitgray}{RGB}{75, 85, 99}

% Custom title page (clean, technical style)
\renewcommand{\maketitle}{
  \begin{titlepage}
    \vspace*{2cm}
    
    % Logo/Title area
    \begin{center}
      {\fontsize{48}{52}\selectfont\bfseries\color{spacekitprimary} SpaceKit}\\[0.5cm]
      {\color{spacekitgray}\rule{0.5\textwidth}{0.5pt}}
    \end{center}
    
    \vspace{2cm}
    
    % Main title
    \begin{center}
      {\fontsize{24}{28}\selectfont\bfseries The Decentralized Infrastructure Platform}\\[0.4cm]
      {\fontsize{24}{28}\selectfont\bfseries (Quantum-Safe Cloud Services)}\\[1cm]
      {\fontsize{14}{16}\selectfont\color{spacekitgray} Quantum-Safe Decentralized Cloud Services for Compute, Storage, Messaging, and AI}
    \end{center}
    
    \vspace{2cm}
    
    % Version info
    \begin{center}
      {\large\bfseries Version 1.1}\\[0.3cm]
      {\color{spacekitaccent} Public Testnet; Mainnet Audit-Gated}
    \end{center}
    
    \vfill
    
    % Metadata
    \begin{center}
      {\large Astor Rivera}\\[0.2cm]
      {\color{spacekitgray} CTO @ SWTCH Labs LLC}\\[0.5cm]
      {\color{spacekitgray} August 18, 2026}\\[0.5cm]
      {\color{spacekitaccent}\texttt{https://spacekit.xyz}}
    \end{center}
    
    \vspace{1cm}
    
    % Footer
    \begin{center}
      {\color{spacekitgray}\rule{0.8\textwidth}{0.4pt}}\\[0.3cm]
      {\footnotesize\color{spacekitgray} Technical Whitepaper}
    \end{center}
    
  \end{titlepage}
}
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

# Clean up temp files
rm -f /tmp/header.tex

# Check if compilation was successful
if [ $PANDOC_EXIT -eq 0 ] && [ -f "${OUTPUT_FILE}" ]; then
    echo ""
    echo -e "${GREEN}Success! PDF generated: ${OUTPUT_FILE}${NC}"
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
    echo "Opening PDF..."
    open "${OUTPUT_FILE}" 2>/dev/null || xdg-open "${OUTPUT_FILE}" 2>/dev/null || echo "Please open ${OUTPUT_FILE} manually"
else
    echo ""
    echo -e "Compilation failed. Check errors above."
    exit 1
fi

