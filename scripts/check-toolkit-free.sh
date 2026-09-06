#!/usr/bin/env bash
# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
#
# `rust/zaparoo-app` is the toolkit-agnostic application layer. It exists so the
# rules the frontend enforces survive the move off Qt: `rust/frontend` is a
# cxx-qt adapter over it, and a future Slint shell would be a second adapter.
# That only holds if the crate never learns about a toolkit, and the cheapest
# way to keep it honest over months of work is to fail the lint gate the moment
# it does. See docs/qt-to-rust-extraction.md.
#
# Deliberately pure coreutils, no cargo, so it runs on any host and in any
# container. Comments are stripped before matching: the crate's own doc
# comments name the banned tokens precisely because they are banned.
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
repo_root="$(cd -- "$script_dir/.." && pwd -P)"
crate="$repo_root/rust/zaparoo-app"

fail() {
    echo "$1" >&2
    echo "rust/zaparoo-app must not depend on a UI toolkit. Put toolkit-facing" >&2
    echo "code in rust/frontend/src/models/ instead; see docs/qt-to-rust-extraction.md." >&2
    exit 1
}

[[ -d "$crate" ]] || fail "Missing $crate."

# Types and helpers that can only come from a toolkit binding.
banned='cxx|slint|QString|QVariant|QStringList|QList|QColor|QByteArray|QModelIndex|qt_thread|qobject|Pin<&mut'

hits="$(
    while IFS= read -r -d '' file; do
        # Strip line comments so prose naming these tokens does not trip.
        sed 's|//.*||' "$file" | grep -nE "$banned" | sed "s|^|${file#"$repo_root/"}:|" || true
    done < <(find "$crate/src" "$crate/tests" -name '*.rs' -type f -print0 2>/dev/null)
)"

if [[ -n "$hits" ]]; then
    echo "Toolkit references found in rust/zaparoo-app:" >&2
    echo "$hits" >&2
    fail "The crate must stay toolkit-agnostic."
fi

# A dependency is the other way the boundary erodes, and it would not show up
# above until something actually used it.
if sed 's/#.*//' "$crate/Cargo.toml" | grep -qiE '^[[:space:]]*(cxx[a-z-]*|slint[a-z-]*)[[:space:]]*='; then
    echo "rust/zaparoo-app/Cargo.toml declares a toolkit dependency:" >&2
    sed 's/#.*//' "$crate/Cargo.toml" | grep -inE '^[[:space:]]*(cxx[a-z-]*|slint[a-z-]*)[[:space:]]*=' >&2
    fail "The crate must stay toolkit-agnostic."
fi
