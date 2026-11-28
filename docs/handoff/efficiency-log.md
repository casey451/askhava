# Efficiency log (D-5980)

One line per finished attempt, newest last: task, attempt, result, worker tokens / turns, reviewer tokens, estimate, waste found, change made.

- T749 a1 (Nov 27, Sonnet): check_failed only on the td_sec_44 chance failure (action_links_route.rs:546), worker 0.40M / 15 turns, reviewer 0.21M. Waste: the whole attempt (second td_sec_44 loss today, after T657 a3). Change: T811, which makes the assert exact, goes second in claim_first right after T871 (D-5981).
- T856 a2 (Nov 27, Opus): handed in after one review fix round; worker 11.85M / 124 turns, reviewer 2.34M; vs intake first-try median 5.23M / 87 + 1.03M (about 2.2x). Pacer estimate 1.9M was far low. Waste: none found beyond size (a wide consent change across replies/web_chat/service). Change: none to the task; T857/T865 drafts re-bound to T856's code before claim (D-5982), which caught a draft instruction that would not compile.
- T871 a1 (Nov 27, Sonnet trial 1, D-5981): reached MERGE_PENDING on attempt 1 (merge CI pending at 23:06Z); worker Sonnet 1.81M / 32 turns, reviewer Opus 0.51M, draft Opus ~0.14M; total 2.46M vs bar 3.7M (-34%); turns 32 vs 55; Opus tokens 0.65M vs comparison Opus median 3.68M (-82%); Opus share 26%. Waste: none found. Change: none; trial continues (T857, T865).
