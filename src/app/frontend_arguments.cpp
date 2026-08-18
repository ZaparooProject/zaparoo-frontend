// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

#include "frontend_arguments.h"

#include <algorithm>
#include <cstddef>
#include <cstring>
#include <iterator>

namespace zaparoo
{
ParsedArguments parseArguments(int argc, char* argv[])
{
    ParsedArguments parsed;
    parsed.argv.reserve(static_cast<size_t>(argc));
    std::copy_n(argv, argc, std::back_inserter(parsed.argv));
    parsed.originalArgv = parsed.argv;
    parsed.originalArgv.push_back(nullptr);

    std::vector<char*> filtered;
    filtered.reserve(parsed.argv.size() + 1);
    if (!parsed.argv.empty())
    {
        filtered.push_back(parsed.argv.front());
    }

    bool optionsEnded = false;
    for (size_t i = 1; i < parsed.argv.size(); ++i)
    {
        char* arg = parsed.argv.at(i);
        if (!optionsEnded && std::strcmp(arg, "--") == 0)
        {
            optionsEnded = true;
            filtered.push_back(arg);
            continue;
        }
        if (!optionsEnded && std::strcmp(arg, "--crt") == 0)
        {
            parsed.crtNativePathForced = true;
            continue;
        }
        if (!optionsEnded && (std::strcmp(arg, "--version") == 0 || std::strcmp(arg, "-v") == 0))
        {
            parsed.versionRequested = true;
        }
        filtered.push_back(arg);
    }

    parsed.argv = std::move(filtered);
    parsed.argv.push_back(nullptr);
    return parsed;
}
} // namespace zaparoo
