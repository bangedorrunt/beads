#!/usr/bin/env bash
# Sourced by scripts/verify-mutations.sh.
# A lower wave gates the waves above it when it is READY work. An expired timed
# deferral is ready (the ready query surfaces it while its status is still
# deferred); a manual deferral with no clock is not.

CARGO_ARGS=(--test storage_ready)
TEST_FILTER=(ready_holds_wave_2_while_wave_1_timed_deferral_expired a_manual_deferral_does_not_gate_the_wave_above)

SQLITE=src/storage/sqlite.rs

mutation "W1  an expired timed deferral stops gating the wave above it" \
    file=$SQLITE \
    expect=ready_holds_wave_2_while_wave_1_timed_deferral_expired \
    find="('open', 'in_progress', 'blocked', 'deferred')" \
    replace="('open', 'in_progress', 'blocked')"

mutation "W2  a manual deferral gates the wave above it forever" \
    file=$SQLITE \
    expect=a_manual_deferral_does_not_gate_the_wave_above \
    find="(lower_wave.status <> 'deferred' OR lower_wave.defer_until IS NOT NULL)" \
    replace="1"
