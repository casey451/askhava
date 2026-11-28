# PF-CHAIN2 Opus verification (Nov 27, origin/main 8ff8dd521)
Counts: 18 CONFIRMED, 10 CONFIRMED-LOWER, 0 WRONG. Report's "all five at owned-file maximum" is wrong (validate_tasks.py:201-203,224: build 10, harness 14; T858 6/10, T859 7/10, T860 6/10, T866 5/14, T801 14/14). Real limit is context (build 45%; T858 40%; service.rs 53 KB ~7%, order.rs 28 KB ~4%).
order.rs exists (732 lines): ask_last_question -> Option<Queued> (:282); show_built_card commits only on Shown::Texted (:329-356); choose_with_card (:359); spanish_line queue_text (:381-400); bank_turn (:559). Unowned by the five.
#1 CONFIRMED service.rs:699-703,729-749,766-804 timing transitions + queue; T857 draft :91 leaves service queue out. BLOCKER.
#2 LOWER intake.md:64-72 regular has no step 7: golden list per path.
#3 CONFIRMED photos.rs:643 -> order::ask_last_question, order.rs:299 -> ask_name_address.
#4 LOWER photos.rs:560-621 markers need a Queued; T858 owns photos.rs: acceptance note.
#5 LOWER web_chat.rs:786-790 consent text queued until T801: "no new homeowner outbox rows after the code".
#6 LOWER chat_codes.rs:63,285,340 multipart/CSRF: T859's own files.
#7 LOWER photos.rs:260-290 contact-only binding: bind session/request/job in T859's web_dispatch.rs.
#8 LOWER site/chat.rs:439-445: acceptance wording.
#9 CONFIRMED order_dispatch.rs:513-538 card build, :545-574 EDIT/SEND, replies.rs:480-482; T860 barred from order_dispatch.rs. BLOCKER.
#10 CONFIRMED cards_out.rs:422-423 after commit, :554 skips at count 0.
#11 CONFIRMED summary_step.rs:187-198; CardTimer literals in unowned tests outbox_end_states.rs:347, tests/integration/tests/summary_step.rs:137,302, intake_order_card.rs:177: keep fields; add a page timer type.
#12 LOWER picture_text.rs:421, cards_out.rs:554: per-request gate; rest folds into #10.
#13 CONFIRMED T866.md:9 vs INT-038 (intake.md:42), TD-CON-36; screen() web_chat.rs:579-592. Verified-STOP part is T801's. BLOCKER.
#14 LOWER web_chat.rs:622-648,680-711: T866 own files; acceptance (stale number/code posts refused, recheck after Verify).
#15 CONFIRMED web_chat_stops.rs:391 ours(&page).is_empty() not in T801 list (:416,:474 fine). BLOCKER.
#16 CONFIRMED and worse: service.rs:584-585 match_reply queues INT-004 with no request_text guard.
#17 CONFIRMED web_chat.rs:773-778 existing-request branch writes no record.
#18 LOWER web_chat.rs:335-346 T801 own: sink UnknownRequest -> handoff screen.
#19 CONFIRMED real gate picture_text.rs:421 (T859 cites :356; T857 draft too).
#20 CONFIRMED web_chat.rs:341-345, :784-794, :464-492.
#21 CONFIRMED web dispatch is T864's.
#22 CONFIRMED summary_step.rs:186-198.
#23 CONFIRMED hold reads text_queued_at service.rs:731-734, models.rs:537; order_dispatch.rs:1569-1586.
#24 CONFIRMED TD-CON-36 (test-design.md:867) requires TXT-041 line on page; web_chat_stops.rs:585-608 calls check_code directly: extend to real code POST.
#25 CONFIRMED answer_list handlers.rs:441 unowned in the chain; web_chat_after_code.rs:368.
#26 CONFIRMED site/chat.rs:307-318 fresh chat per GET, said capped 20 (:53).
#27 CONFIRMED TXT-032 "We'll text you" vs TXT-034 email (messaging.md:36,38).
#28 LOWER request.html:26,50-75 render states + copy (WORD-028).
Seams: S1 timing+safety (service.rs, order.rs, conversation.rs, models.rs, hold_911 in order_dispatch.rs, new test) after T864 before T858: #1,#3,#16 safety,#23. S2 card+address+LIST (order_dispatch.rs, replies.rs, handlers.rs, order.rs) after T858 before T860: #9,#16 address,#25. T859: #6,#7,#8,#12 picture,#19,#21. T860: #10,#11,#12 cards-out,#22,#26,#27. T866: #13,#14,#28. T801: #5,#15,#17,#18,#20,#24. T858: #2,#4.
