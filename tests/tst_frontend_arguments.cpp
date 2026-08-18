// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

#include "frontend_arguments.h"

#include <cstddef>
#include <cstdio>
#include <iterator>
#include <string_view>
#include <vector>

namespace
{
bool check(bool condition, const char* message)
{
    if (!condition)
    {
        std::fprintf(stderr, "frontend argument test failed: %s\n", message);
    }
    return condition;
}

bool matches(const std::vector<char*>& actual, const std::vector<std::string_view>& expected)
{
    if (actual.size() != expected.size() + 1 || actual.back() != nullptr)
    {
        return false;
    }
    for (size_t i = 0; i < expected.size(); ++i)
    {
        if (std::string_view(actual.at(i)) != expected.at(i))
        {
            return false;
        }
    }
    return true;
}
} // namespace

int main()
{
    bool passed = true;

    char program[] = "frontend";
    char* noFlags[] = {program};
    const auto plain = zaparoo::parseArguments(static_cast<int>(std::size(noFlags)), noFlags);
    passed &= check(!plain.crtNativePathForced, "no-flag launch forced CRT");
    passed &= check(!plain.versionRequested, "no-flag launch requested version");
    passed &= check(matches(plain.argv, {"frontend"}), "no-flag argv lacks null sentinel");
    passed &= check(matches(plain.originalArgv, {"frontend"}),
                    "no-flag original argv lacks null sentinel");

    char crt[] = "--crt";
    char value[] = "ordinary";
    char* filteredInput[] = {program, crt, value};
    const auto filtered =
        zaparoo::parseArguments(static_cast<int>(std::size(filteredInput)), filteredInput);
    passed &= check(filtered.crtNativePathForced, "--crt was not recognized");
    passed &= check(!filtered.versionRequested, "ordinary argument requested version");
    passed &= check(matches(filtered.argv, {"frontend", "ordinary"}), "--crt was not filtered");
    passed &= check(matches(filtered.originalArgv, {"frontend", "--crt", "ordinary"}),
                    "original argv did not preserve --crt");

    char version[] = "--version";
    char* versionInput[] = {program, version};
    const auto versioned =
        zaparoo::parseArguments(static_cast<int>(std::size(versionInput)), versionInput);
    passed &= check(versioned.versionRequested, "--version was not recognized");

    char separator[] = "--";
    char shortVersion[] = "-v";
    char* terminatedInput[] = {program, separator, crt, shortVersion};
    const auto terminated =
        zaparoo::parseArguments(static_cast<int>(std::size(terminatedInput)), terminatedInput);
    passed &= check(!terminated.crtNativePathForced, "--crt after -- was consumed");
    passed &= check(!terminated.versionRequested, "-v after -- requested version");
    passed &= check(matches(terminated.argv, {"frontend", "--", "--crt", "-v"}),
                    "arguments after -- were not preserved");

    return passed ? 0 : 1;
}
