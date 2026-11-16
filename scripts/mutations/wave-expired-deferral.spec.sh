#!/usr/bin/env bash
# Sourced by scripts/verify-mutations.sh.
# A lower wave holds the waves above it when it is READY work, and an expired
# timed deferral is ready: the ready query surfaces that row while its status
# is still `deferred`.

CARGO_ARGS=(--test storage_ready)
TEST_FILTER=(ready_holds_wave_2_while_wave_1_timed_deferral_expired)

SQLITE=src/storage/sqlite.rs

mutation "W1  an expired timed deferral stops gating the wave above it" \
    file=$SQLITE \
    expect=ready_holds_wave_2_while_wave_1_timed_deferral_expired \
    find="OR (lower_wave.status = 'deferred' AND lower_wave.defer_until IS NOT NULL))" \
    replace="OR (lower_wave.status = 'deferred' AND 0))"
