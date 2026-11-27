# Efficiency log (D-5980)

One line per finished attempt, newest last: task, attempt, result, worker tokens / turns, reviewer tokens, estimate, waste found, change made.

- T749 a1 (Nov 27, Sonnet): check_failed only on the td_sec_44 chance failure (action_links_route.rs:546), worker 0.40M / 15 turns, reviewer 0.21M. Waste: the whole attempt (second td_sec_44 loss today, after T657 a3). Change: T811, which makes the assert exact, goes second in claim_first right after T871 (D-5981).
