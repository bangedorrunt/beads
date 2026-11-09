---- MODULE BeadsReadyAndBlocked ----
EXTENDS Naturals, TLC

CONSTANT
    FixReadyWaveAndBlockedJson

VARIABLES
    w1_status,
    w1_defer,
    w1_has_open_dep

vars == <<w1_status, w1_defer, w1_has_open_dep>>

LowerWaveGateActive ==
    /\ IF FixReadyWaveAndBlockedJson
       THEN (w1_status \in {"open", "in_progress"} \/ (w1_status = "deferred" /\ w1_defer # "none"))
       ELSE w1_status \in {"open", "in_progress"}
    /\ ~w1_has_open_dep
    /\ w1_defer \in {"none", "expired"}

MinActiveWave ==
    IF LowerWaveGateActive THEN 1 ELSE 999

W1InReady ==
    /\ (w1_status = "open" \/ (w1_status = "deferred" /\ w1_defer # "none"))
    /\ ~w1_has_open_dep
    /\ w1_defer \in {"none", "expired"}
    /\ 1 <= MinActiveWave

W2InReady ==
    2 <= MinActiveWave

MayHaveBlockedCommandResults ==
    IF FixReadyWaveAndBlockedJson
    THEN (w1_has_open_dep \/ w1_status = "blocked")
    ELSE w1_has_open_dep

W1InBlockedJson ==
    IF ~MayHaveBlockedCommandResults
    THEN FALSE
    ELSE (w1_status # "closed" /\ (w1_status = "blocked" \/ w1_has_open_dep))

Init ==
    /\ w1_status = "open"
    /\ w1_defer = "none"
    /\ w1_has_open_dep = FALSE

ClaimW1 ==
    /\ w1_status = "open"
    /\ ~w1_has_open_dep
    /\ w1_defer \in {"none", "expired"}
    /\ w1_status' = "in_progress"
    /\ UNCHANGED <<w1_defer, w1_has_open_dep>>

DeferW1Timed ==
    /\ w1_status \in {"open", "in_progress"}
    /\ w1_status' = "deferred"
    /\ w1_defer' = "future"
    /\ UNCHANGED <<w1_has_open_dep>>

DeferW1Manual ==
    /\ w1_status \in {"open", "in_progress"}
    /\ w1_status' = "deferred"
    /\ w1_defer' = "none"
    /\ UNCHANGED <<w1_has_open_dep>>

ExpireDeferClock ==
    /\ w1_status = "deferred"
    /\ w1_defer = "future"
    /\ w1_defer' = "expired"
    /\ UNCHANGED <<w1_status, w1_has_open_dep>>

UndeferW1 ==
    /\ w1_status = "deferred"
    /\ w1_status' = "open"
    /\ w1_defer' = "none"
    /\ UNCHANGED <<w1_has_open_dep>>

SetExplicitBlockedStatus ==
    /\ w1_status \in {"open", "in_progress"}
    /\ w1_status' = "blocked"
    /\ UNCHANGED <<w1_defer, w1_has_open_dep>>

AddOpenDepBlocker ==
    /\ ~w1_has_open_dep
    /\ w1_status # "closed"
    /\ w1_has_open_dep' = TRUE
    /\ UNCHANGED <<w1_status, w1_defer>>

ResolveDepBlocker ==
    /\ w1_has_open_dep
    /\ w1_has_open_dep' = FALSE
    /\ UNCHANGED <<w1_status, w1_defer>>

CloseW1 ==
    /\ w1_status = "in_progress"
    /\ ~w1_has_open_dep
    /\ w1_status' = "closed"
    /\ w1_defer' = "none"
    /\ UNCHANGED <<w1_has_open_dep>>

Done ==
    /\ w1_status = "closed"
    /\ UNCHANGED vars

Next ==
    \/ ClaimW1
    \/ DeferW1Timed
    \/ DeferW1Manual
    \/ ExpireDeferClock
    \/ UndeferW1
    \/ SetExplicitBlockedStatus
    \/ AddOpenDepBlocker
    \/ ResolveDepBlocker
    \/ CloseW1
    \/ Done

Spec == Init /\ [][Next]_vars

WaveOrderingPreservedInReady ==
    W1InReady => ~W2InReady

ExplicitBlockedVisibleInBlockedJson ==
    (w1_status = "blocked") => W1InBlockedJson

====
