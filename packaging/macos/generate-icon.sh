#!/bin/bash
# ABOUTME: Generate placeholder app icon for gorp
# ABOUTME: Creates a simple icon using sips (macOS built-in)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ICONSET_DIR="$SCRIPT_DIR/gorp.iconset"
ICON_OUTPUT="$SCRIPT_DIR/gorp.icns"

echo "Generating placeholder icon..."

# Create iconset directory
rm -rf "$ICONSET_DIR"
mkdir -p "$ICONSET_DIR"

# Check if we have a source PNG
if [[ -f "$SCRIPT_DIR/icon-source.png" ]]; then
    SOURCE_PNG="$SCRIPT_DIR/icon-source.png"
    echo "Using source: $SOURCE_PNG"
else
    # Create a simple placeholder using Python (if available)
    if command -v python3 &> /dev/null; then
        echo "Creating placeholder icon with Python..."
        cd "$SCRIPT_DIR"
        python3 << 'EOF'
from PIL import Image, ImageDraw, ImageFont
import os
size = 1024
img = Image.new('RGBA', (size, size), (30, 30, 30, 255))
draw = ImageDraw.Draw(img)

# Draw a rounded rectangle background
margin = 100
draw.rounded_rectangle(
    [margin, margin, size - margin, size - margin],
    radius=150,
    fill=(45, 45, 45, 255),
    outline=(100, 100, 255, 255),
    width=8
)

# Draw "gorp" text
try:
    font = ImageFont.truetype("/System/Library/Fonts/Helvetica.ttc", 280)
except:
    font = ImageFont.load_default()

text = "gorp"
bbox = draw.textbbox((0, 0), text, font=font)
text_width = bbox[2] - bbox[0]
text_height = bbox[3] - bbox[1]
x = (size - text_width) // 2
y = (size - text_height) // 2 - 50

draw.text((x, y), text, fill=(100, 150, 255, 255), font=font)

img.save('icon-source.png')
print("Created icon-source.png")
EOF
        SOURCE_PNG="icon-source.png"
    else
        echo "Error: No source icon and Python/Pillow not available"
        echo "Please provide icon-source.png (1024x1024 PNG) in $SCRIPT_DIR"
        exit 1
    fi
fi

# Generate all required icon sizes
SIZES=(16 32 64 128 256 512 1024)

for size in "${SIZES[@]}"; do
    sips -z $size $size "$SOURCE_PNG" --out "$ICONSET_DIR/icon_${size}x${size}.png" > /dev/null

    # Create @2x versions for Retina
    if [[ $size -le 512 ]]; then
        double=$((size * 2))
        sips -z $double $double "$SOURCE_PNG" --out "$ICONSET_DIR/icon_${size}x${size}@2x.png" > /dev/null
    fi
done

# Convert iconset to icns
iconutil -c icns "$ICONSET_DIR" -o "$ICON_OUTPUT"

# Clean up
rm -rf "$ICONSET_DIR"
if [[ -f "icon-source.png" ]] && [[ ! -f "$SCRIPT_DIR/icon-source.png" ]]; then
    mv icon-source.png "$SCRIPT_DIR/"
fi

echo "Icon created: $ICON_OUTPUT"
