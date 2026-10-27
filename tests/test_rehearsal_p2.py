"""K9 rehearsal practice task P2: fails on purpose so the runner can prove that a red
task is retried, then BLOCKED, and never merges. It is never meant to reach main."""


def test_rehearsal_p2_fails_on_purpose() -> None:
    assert 1 + 1 == 3, "K9 rehearsal P2: deliberate failure"
