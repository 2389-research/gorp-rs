#!/bin/bash
# ABOUTME: Fix .mcp.json files that have incorrect external ports
# ABOUTME: Changes localhost:130XX back to localhost:13000 (internal container port)

set -e

WORKSPACE_DIR="${1:-/home/gorp/workspace}"

echo "Fixing .mcp.json ports in: $WORKSPACE_DIR"

# Find all .mcp.json files and fix ports
found=0
fixed=0

while IFS= read -r -d '' mcp_file; do
    found=$((found + 1))
    # Check if file has wrong port (130XX where XX > 00)
    if grep -qE 'localhost:130[0-9][1-9]' "$mcp_file" 2>/dev/null; then
        echo "  Fixing: $mcp_file"
        sed -i 's/localhost:130[0-9][0-9]/localhost:13000/g' "$mcp_file"
        fixed=$((fixed + 1))
    fi
done < <(find "$WORKSPACE_DIR" -name ".mcp.json" -print0 2>/dev/null)

echo ""
echo "Found $found .mcp.json files"
echo "Fixed $fixed files"

if [ $fixed -eq 0 ]; then
    echo "All ports already correct!"
fi
