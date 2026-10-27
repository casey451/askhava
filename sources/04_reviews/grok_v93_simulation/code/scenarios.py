"""Part 10 scenarios (+ my own S17 set) as config overrides. All values are Assumed unless the brief gives them (Given)."""
from config import POOL, CATS

GROWTH = {"m1": 1, "m12": 10, "m24": 16}                      # G: Part 10 #2
LICENSED = ["plumbing", "hvac_repair", "hvac_replace", "electrical"]   # G: approved hand-run test scope (licensed trades)
NEW_GROUPS = ["pool", "landscape", "cleaning", "pest", "boat", "detail", "handyman", "movers", "junk", "trashcan", "dogwaste",
              "window", "pwash", "solar"]

def thin_override():
    ov = {}
    half = NEW_GROUPS[::2]                                   # A: every other new category has a single business
    for g in half: ov[g] = 1
    ov["dogwaste"] = 0; ov["junk"] = 0                       # G
    return ov

SCEN = {
 "S01_normal":        {},
 "S02_growth":        {"days": 730, "growth": GROWTH},
 "S03_busy_summer":   {"requests_per_day": 16, "start_month": 7, "heat_wave": (20, 7, 3.0)},
 "S04_snowbird":      {"start_month": 1, "snowbird": True, "p_out_of_state_phone": 0.45},
 "S05_boat_season":   {"start_month": 5, "season": {"hvac_summer_mult": 3.0, "boat_season_mult": 4.0, "snowbird_clean_mult": 1.5}},
 "S06_vendor_chaos":  {"chaos": 3.0, "stripe_outage": (25, 4), "ai_outage": (35, 6)},
 "S07_twilio_suspend":{"tw": {"suspend_day": 30, "suspend_days": 30}},
 "S08_adversarial":   {"adversarial_mult": 2.0},
 "S09_casey_away":    {"casey": {"away": (20, 14)}},
 "S10_dispute_wave":  {"dispute_wave": (25, 5)},
 "S11a_collide_fwd":  {"requests_per_day": 16, "collision": "fwd"},
 "S11b_collide_rev":  {"requests_per_day": 16, "collision": "rev"},
 "S12_db_restore":    {"restore": {"day": 30, "lost_h": 3, "mode": "spec"}},
 "S13_crowded":       {"per_category_override": {g: 15 for g in POOL}},
 "S14_thin":          {"per_category_override": thin_override()},
 "S15_spanish20":     {"spanish_share_override": 0.20},
 # ---- S17: my choices
 "S17a_all_fixes":    {"fix": {k: True for k in ["code_scrub_before_ai", "no_photos_on_cards", "stable_list_numbers", "pick_idempotency_key",
                        "quiet_cutoff_2030", "spanish_optout", "consent_lenient", "pin_when_multi_job", "never_mind_cancel", "safety_followup",
                        "window_matches_promise", "handyman_disclosure", "casey_checklist", "emergency_indirect_keywords", "permit_question"]}},
 "S17b_approved_scope":{"approved_scope": True, "categories": LICENSED, "fees": "approved_kit"},
 "S17c_weekday_window":{"quote_window": "weekdays"},
 "S17d_roc_lists":    {"roc_mode": "lists"},
 "S17e_insurance_req":{"insurance_required": True},
 "S17f_rec_partial":  {"recurring_partial_refund": True},
 "S17g_roc_layout":   {"roc": {"forced_layout_day": 20}},
 "S17h_10_per_day":   {"requests_per_day": 10},
}
FEE_STRUCTS = ["base", "alt1", "alt2", "alt3", "alt4", "alt5"]
S50S = [0.10, 0.15, 0.20]
