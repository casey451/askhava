"""The build runner's decision logic (BUILD_BRIEF.md section 7), standard library only.

Everything here is pure or takes its outside world (git, GitHub, the worker session)
through the small protocols in runner.cycle, so the failure drills can run offline.
"""
