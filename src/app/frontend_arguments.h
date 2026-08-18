// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

#pragma once

#include <vector>

namespace zaparoo
{
struct ParsedArguments
{
    bool crtNativePathForced = false;
    bool versionRequested = false;
    std::vector<char*> argv;
    // Unfiltered process arguments (nullptr-terminated). Restart execvp uses
    // these because argv has frontend-only options stripped before Qt.
    std::vector<char*> originalArgv;
};

ParsedArguments parseArguments(int argc, char* argv[]);
} // namespace zaparoo
