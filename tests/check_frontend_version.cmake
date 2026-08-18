# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

set(_args "${ARG1}")
if(DEFINED ARG2)
    list(APPEND _args "${ARG2}")
endif()

execute_process(
    COMMAND "${FRONTEND}" ${_args}
    RESULT_VARIABLE _result
    OUTPUT_VARIABLE _stdout
    ERROR_VARIABLE _stderr
)

if(NOT _result EQUAL 0)
    message(FATAL_ERROR "frontend version command exited ${_result}: ${_stderr}")
endif()
if(NOT _stdout STREQUAL "Zaparoo Frontend ${EXPECTED}\n")
    message(FATAL_ERROR "unexpected frontend version stdout: '${_stdout}'")
endif()
if(NOT _stderr STREQUAL "")
    message(FATAL_ERROR "frontend version command initialized diagnostics: '${_stderr}'")
endif()
