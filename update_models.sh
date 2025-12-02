#!/bin/bash
set -e

# Configuration
MODEL_DIR="model"
CHECK_FILE="$MODEL_DIR/.model_checksums"
API_URL="https://api.github.com/repos/86maid/ddddocr/contents/model?ref=master"
RAW_BASE="https://raw.githubusercontent.com/86maid/ddddocr/master/model"
FILES=("common.onnx" "common.json" "common_det.onnx")

# Ensure model directory exists
mkdir -p "$MODEL_DIR"

# Clean up deprecated files
rm -f "$MODEL_DIR/common_old.onnx" "$MODEL_DIR/common_old.json"

echo "Fetching remote model metadata..."
# Fetch directory listing from GitHub API to get Blob SHAs
REMOTE_JSON=$(curl -sL "$API_URL")

# Check if we got a valid JSON response
if [[ "$REMOTE_JSON" != *"["* ]]; then
    echo "Error: Failed to fetch metadata from GitHub API."
    echo "Response: $REMOTE_JSON"
    exit 1
fi

# Helper function to extract SHA from JSON using grep/sed
get_remote_sha() {
    local fname=$1
    # We need to find the block that contains "name": "fname" and extract "sha" from it.
    # Since the JSON is pretty printed or at least structured, we can't rely on single line match.
    # But we can assume the structure "name": "...", ... "sha": "..." is within a reasonable distance or standard order?
    # Actually, in the output provided: "name" comes first, then "path", then "sha".
    # We can use grep -A to look after name match.
    
    # Strategy:
    # 1. grep for line with "name": "fname", include following lines
    # 2. grep the first "sha" occurrence
    # 3. extract value
    
    echo "$REMOTE_JSON" | grep -A 5 "\"name\": \"$fname\"" | grep "\"sha\":" | head -n 1 | cut -d'"' -f4
}

# Helper function to get local SHA from check file
get_local_sha() {
    local fname=$1
    if [ -f "$CHECK_FILE" ]; then
        # Format: filename sha
        grep "^$fname " "$CHECK_FILE" | awk '{print $2}'
    fi
}

# Process each file
for fname in "${FILES[@]}"; do
    remote_sha=$(get_remote_sha "$fname")
    local_sha=$(get_local_sha "$fname")
    
    if [ -z "$remote_sha" ]; then
        echo "Warning: Could not find remote SHA for $fname. Skipping."
        continue
    fi

    # Check if update is needed
    # Update if: Local SHA mismatch OR File missing
    if [ "$remote_sha" == "$local_sha" ] && [ -f "$MODEL_DIR/$fname" ]; then
        echo "[OK] $fname is up to date."
    else
        if [ -f "$MODEL_DIR/$fname" ]; then
            echo "[UPDATE] $fname (Local: ${local_sha:0:7} -> Remote: ${remote_sha:0:7})..."
        else
            echo "[DOWNLOAD] $fname (SHA: ${remote_sha:0:7})..."
        fi
        
        # Download file
        curl -L --retry 3 -o "$MODEL_DIR/$fname" "$RAW_BASE/$fname"
        
        # Verify integrity if possible? No, we just trust download and store SHA.
        
        # Update check file
        touch "$CHECK_FILE"
        # Remove old entry
        grep -v "^$fname " "$CHECK_FILE" > "$CHECK_FILE.tmp" || true
        mv "$CHECK_FILE.tmp" "$CHECK_FILE"
        # Append new entry
        echo "$fname $remote_sha" >> "$CHECK_FILE"
    fi
done

echo "Model update completed."