#!/bin/bash

# Update CHANGELOG.md incrementally based on commits since last-commit marker.
# Also bumps the version in Cargo.toml.
#
# Usage:
#   ./scripts/update-changelog.sh              # auto-increment patch version
#   ./scripts/update-changelog.sh v1.2.0       # use specified version

set -e

CHANGELOG="CHANGELOG.md"
CARGO="Cargo.toml"

# --- Version ---
CURRENT=$(grep '^version' "$CARGO" | head -1 | sed 's/.*= *//' | tr -d '"')

if [ -n "$1" ]; then
    # Strip leading 'v' for Cargo.toml, keep it for display
    NEW_RAW="${1#v}"
    VERSION="v${NEW_RAW}"
    echo "📌 Using specified version: $VERSION"
else
    # Auto-increment patch: 1.0.0 -> 1.0.1
    MAJOR=$(echo "$CURRENT" | cut -d. -f1)
    MINOR=$(echo "$CURRENT" | cut -d. -f2)
    PATCH=$(echo "$CURRENT" | cut -d. -f3)
    NEW_RAW="${MAJOR}.${MINOR}.$((PATCH + 1))"
    VERSION="v${NEW_RAW}"
    echo "📌 Auto-incrementing: v${CURRENT} → ${VERSION}"
fi

# --- Update Cargo.toml ---
sed -i '' "s/^version = \"${CURRENT}\"/version = \"${NEW_RAW}\"/" "$CARGO"
echo "✅ Cargo.toml: version = \"${NEW_RAW}\""

DATE=$(date +%Y-%m-%d)

# --- Find last synced commit ---
LAST_COMMIT=$(grep 'last-commit:' "$CHANGELOG" | sed 's/.*last-commit: *//' | tr -d ' -->' | head -1)

if [ -z "$LAST_COMMIT" ]; then
    echo "❌ No '<!-- last-commit: SHA -->' marker found in $CHANGELOG"
    exit 1
fi

# --- Get new commits since last sync ---
NEW_COMMITS=$(git log --pretty=format:"%h %s" "${LAST_COMMIT}..HEAD")

if [ -z "$NEW_COMMITS" ]; then
    echo "⚠️  No new commits since $LAST_COMMIT"
fi

# --- Build new section ---
LATEST_SHA=$(git rev-parse --short HEAD)

COMMIT_ROWS=""
while IFS= read -r line; do
    [ -z "$line" ] && continue
    SHA=$(echo "$line" | cut -d' ' -f1)
    MSG=$(echo "$line" | cut -d' ' -f2-)
    COMMIT_ROWS="${COMMIT_ROWS}| \`${SHA}\` | ${MSG} |\n"
done <<< "$NEW_COMMITS"

NEW_SECTION="## ${VERSION} - ${DATE}\n\n| Commit | Description |\n|--------|-------------|\n${COMMIT_ROWS}"

# --- Prepend new section after the last-commit marker line ---
MARKER_LINE=$(grep -n 'last-commit:' "$CHANGELOG" | head -1 | cut -d: -f1)
BEFORE=$(head -n "$MARKER_LINE" "$CHANGELOG")
AFTER=$(tail -n +"$((MARKER_LINE + 1))" "$CHANGELOG")

printf '%s\n\n%b\n%s\n' "$BEFORE" "$NEW_SECTION" "$AFTER" > "$CHANGELOG"

# --- Update last-commit marker ---
sed -i '' "s/last-commit: ${LAST_COMMIT}/last-commit: ${LATEST_SHA}/" "$CHANGELOG"

echo "✅ CHANGELOG.md updated"
echo "   Marker:  $LAST_COMMIT → $LATEST_SHA"

# --- Commit and push ---
git add "$CHANGELOG" "$CARGO"
cargo build --release
git add .
git commit -S -m "update CHANGELOG.md"
echo "🚀 update CHANGELOG.md"
