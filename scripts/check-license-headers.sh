#!/usr/bin/env bash
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.
#
# Copyright (c) 2024-2026 2pisoftware Pty Ltd

# check-license-headers.sh
# Verifies that all source files contain the MPL-2.0 license header.
# Run this in CI or as a pre-commit hook.

set -euo pipefail

EXPECTED="Mozilla Public"
MISSING=()

# Check Rust files
while IFS= read -r -d '' file; do
    if ! head -5 "$file" | grep -q "$EXPECTED"; then
        MISSING+=("$file")
    fi
done < <(find . -name "*.rs" -not -path "*/target/*" -print0)

# Check Rhai scripts
while IFS= read -r -d '' file; do
    if ! head -5 "$file" | grep -q "$EXPECTED"; then
        MISSING+=("$file")
    fi
done < <(find . -name "*.rhai" -not -path "*/target/*" -print0)

# Check TypeScript/JavaScript
while IFS= read -r -d '' file; do
    if ! head -5 "$file" | grep -q "$EXPECTED"; then
        MISSING+=("$file")
    fi
done < <(find . -name "*.ts" -o -name "*.tsx" -o -name "*.js" -o -name "*.jsx" \
    | grep -v node_modules | grep -v dist | tr '\n' '\0')

if [ ${#MISSING[@]} -gt 0 ]; then
    echo "ERROR: The following files are missing the MPL-2.0 license header:"
    echo ""
    for file in "${MISSING[@]}"; do
        echo "  $file"
    done
    echo ""
    echo "Add this header to the top of each file:"
    echo ""
    echo '  // This Source Code Form is subject to the terms of the Mozilla Public'
    echo '  // License, v. 2.0. If a copy of the MPL was not distributed with this'
    echo '  // file, You can obtain one at https://mozilla.org/MPL/2.0/.'
    echo '  //'
    echo '  // Copyright (c) 2024-2026 TripleACS Pty Ltd t/a 2pi Software'
    echo ""
    exit 1
else
    echo "All source files have the MPL-2.0 license header."
fi
