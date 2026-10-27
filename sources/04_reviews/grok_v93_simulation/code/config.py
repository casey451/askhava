"""Havasu Home Quotes v9.3 simulator: default configuration.
Every value carries a label: G = Given (stated in the brief), A = Assumed (chosen by the
simulator or by the brief as a behaviour rate). Measured values only come out of runs.
All numbers are simulated; no real person, business or service is involved."""
import copy, json

# ---- 25 categories (Part 4.1, G: share, typical value, recurring, license rule) ----
# lic: None = no license; tuple of acceptable ROC classes; 'PEST' = AZ Dept of Agriculture.
# hm: handyman exemption may apply to non-gas / non-permit / non-fire job types (G, Part 4.1 table).
CATS = {
 #  key            share  value  recur  lic                   hm     group        trip_tier
 "plumbing":      (0.13,  300,  False, ("R-37",),             True,  "plumbing",  "small"),
 "hvac_repair":   (0.12,  450,  False, ("R-39", "R-39R"),     False, "hvac",      "small"),
 "hvac_replace":  (0.02, 9000,  False, ("R-39", "R-39R"),     False, "hvac",      "large"),
 "electrical":    (0.07,  350,  False, ("R-11",),             False, "electrical","small"),
 "pool_repair":   (0.04,  500,  False, ("R-6", "B-6"),        True,  "pool",      "small"),
 "handyman":      (0.08,  250,  False, None,                  True,  "handyman",  "small"),
 "pool_cleaning": (0.06, 1400,  True,  None,                  False, "pool",      "small"),
 "landscape":     (0.05, 2000,  True,  None,                  False, "landscape", "small"),
 "house_cleaning":(0.05, 3500,  True,  None,                  False, "cleaning",  "small"),
 "pest":          (0.04,  500,  True,  "PEST",                False, "pest",      "small"),
 "dog_waste":     (0.01,  600,  True,  None,                  False, "dogwaste",  "small"),
 "trash_can":     (0.01,  300,  True,  None,                  False, "trashcan",  "small"),
 "turnover":      (0.02, 4000,  True,  None,                  False, "cleaning",  "small"),
 "junk":          (0.03,  250,  False, None,                  False, "junk",      "small"),
 "car_detail":    (0.03,  250,  False, None,                  False, "detail",    "small"),
 "boat_detail":   (0.03,  700,  False, None,                  False, "detail",    "small"),
 "boat_repair":   (0.04,  900,  False, None,                  False, "boat",      "medium"),
 "window":        (0.02,  175,  False, None,                  False, "window",    "small"),
 "pressure_wash": (0.02,  350,  False, None,                  False, "pwash",     "small"),
 "solar_screens": (0.02, 1500,  False, None,                  False, "solar",     "medium"),
 "roof":          (0.02, 1100,  False, ("R-42",),             False, "roof",      "medium"),
 "painting":      (0.03, 2000,  False, ("R-34",),             False, "painting",  "medium"),
 "movers":        (0.02, 1200,  False, None,                  False, "movers",    "medium"),
 "appliance":     (0.02,  250,  False, ("R-63",),             True,  "appliance", "small"),
 "garage_door":   (0.02,  350,  False, ("ROC-any",),          True,  "garage",    "small"),
}
# Job types: (name, weight, gas, permit, fire, lic_override). A: weights and flags.
JOBTYPES = {
 "plumbing":     [("leak",.40,0,0,0,None),("toilet",.20,0,0,0,None),("drain",.18,0,0,0,None),
                  ("water_heater_gas",.10,1,1,0,None),("water_heater_elec",.07,0,1,0,None),("gas_line",.05,1,1,0,None)],
 "hvac_repair":  [("ac_not_cooling",.60,0,0,0,None),("furnace_gas",.15,1,0,0,None),("thermostat",.15,0,0,0,None),("duct",.10,0,0,0,None)],
 "hvac_replace": [("system_replace",1,0,1,0,None)],
 "electrical":   [("outlet",.40,0,0,0,None),("breaker",.30,0,0,0,None),("lighting",.20,0,0,0,None),("panel_upgrade",.10,0,1,0,None)],
 "pool_repair":  [("pump",.50,0,0,0,None),("filter",.20,0,0,0,None),("heater_gas",.15,1,0,0,None),("pool_leak",.15,0,0,0,None)],
 "handyman":     [("drywall",.30,0,0,0,None),("doors",.20,0,0,0,None),("shelves",.20,0,0,0,None),("fence",.15,0,0,0,None),
                  ("smoke_alarm_hardwired",.05,0,0,1,("R-11",)),("gas_dryer_hookup",.05,1,0,0,("R-37",)),("wall_removal",.05,0,1,0,("ROC-any",))],
 "landscape":    [("maintenance",.85,0,0,0,None),("weed_control",.15,0,0,0,"PEST")],
 "pressure_wash":[("wash",.85,0,0,0,None),("seal_paint",.15,0,0,0,("R-34",))],
 "appliance":    [("plugin_appliance",.70,0,0,0,None),("gas_appliance",.30,1,0,0,("R-63",))],
}
# Businesses in town (Part 8.2, G where listed; A where the brief gives no count)
POOL = {"plumbing":(22,22),"hvac":(23,23),"electrical":(31,31),"pool":(19,50),"landscape":(25,47),
        "cleaning":(25,25),"pest":(12,19),"boat":(36,57),"detail":(25,25),"handyman":(7,7),"roof":(18,18),
        "movers":(2,5),"junk":(2,5),"trashcan":(1,1),"dogwaste":(0,0),
        # A: not in the brief
        "window":(6,6),"pwash":(6,6),"solar":(5,5),"painting":(15,15),"appliance":(6,6),"garage":(5,5)}
GROUP_CATS = {"plumbing":["plumbing"],"hvac":["hvac_repair","hvac_replace"],"electrical":["electrical"],
              "pool":["pool_cleaning","pool_repair"],"landscape":["landscape"],"cleaning":["house_cleaning","turnover","window"],
              "pest":["pest","landscape"],"boat":["boat_repair"],"detail":["car_detail","boat_detail"],
              "handyman":["handyman","pressure_wash","junk","window","plumbing","appliance","garage_door"],
              "roof":["roof"],"movers":["movers"],"junk":["junk"],"trashcan":["trash_can"],"dogwaste":["dog_waste"],
              "window":["window"],"pwash":["pressure_wash"],"solar":["solar_screens"],"painting":["painting","pressure_wash"],
              "appliance":["appliance"],"garage":["garage_door"]}
GROUP_LIC = {"plumbing":("R-37",),"hvac":("R-39",),"electrical":("R-11",),"roof":("R-42",),"painting":("R-34",),
             "appliance":("R-63",),"garage":("ROC-any",),"pest":("PEST",)}
# Part 6.3 cost to win a customer elsewhere (G where found; A otherwise)
CAC = {"plumbing":263,"hvac":288,"electrical":218,"roof":369,"handyman":77,"landscape":89,"pool":150,"pest":57,
       "cleaning":100,"junk":200,"boat":150,"detail":80,"movers":200,"trashcan":60,"dogwaste":60,"window":80,
       "pwash":90,"solar":200,"painting":250,"appliance":120,"garage":180}

FEES = {  # Part 6 (G)
 "base":    {"kind":"tier","t":[25,75,199],"rec_months":2},
 "alt1":    {"kind":"tier","t":[19,59,199],"rec_months":2},
 "alt2":    {"kind":"tier","t":[25,75,300],"rec_months":2},
 "alt3":    {"kind":"flat","f":25,"rec_months":2},
 "alt4":    {"kind":"pct","pct":0.05,"min":15,"max":250,"rec_months":2},
 "alt5":    {"kind":"tier","t":[25,75,199],"rec_months":4},
 # Extra structures for comparison (not in Part 6.2):
 "approved_kit": {"kind":"approved","rec_months":2},   # $45 plumb/elec, $59 HVAC, $149 major (approved hand-run test)
 "cat_flat":     {"kind":"catflat","t":[25,75,199],"rec_months":2},  # tier fixed by category, not by quote (proposal)
}

DEFAULT = {
 "label": {},
 "days": 60, "start_month": 10,                    # A: model month-of-year 10 for seasonality only
 "requests_per_day": 5.0,                          # G scenario value
 "growth": None,                                   # {"m1":1,"m12":10,"m24":16}
 "towns": ["Lake Havasu City"],
 "categories": list(CATS),
 "category_share_override": {},
 "season": {"hvac_summer_mult":3.0, "boat_season_mult":2.0, "snowbird_clean_mult":1.5},   # G (x3, x2), A (1.5)
 "heat_wave": None,                                # (start_day, days, hvac_mult)
 "fees": "base", "s50": 0.15,                       # G three settings 0.10/0.15/0.20
 "free_first_win": "all",                          # G; variant "first90"
 "quote_window": "everyday",                       # G open decision; "weekdays"
 "value_sigma": 0.6,                               # G (brief's assumption)
 # ---- homeowners (Part 8.1, G) ----
 "p_zip":0.60, "p_photo":0.50, "p_repeat":0.10, "p_email":0.30,
 "personas": {"normal":.52,"impatient":.06,"nolinks":.06,"spanish":.04,"tenant":.05,"absent":.05,"emergency":.03,
              "oversharer":.06,"forwarder":.04,"screener":.04,"canceller":.03,"injector":.01,"stopreturn":.01},
 "spanish_share_override": None,
 "p_overnight": 0.05,                              # G
 "p_out_of_state_phone": 0.20,                     # A (snowbirds keep home numbers)
 "p_short_first_msg": 0.05,                        # A ("AC broken")
 "p_photo_identifying": 0.20,                      # A share of photos with faces/house numbers/docs
 "p_out_of_area": 0.05,                            # A
 "p_dup_within_2h": 0.05,                          # A second message within 2 h
 "p_direct_contact": 0.06,                         # A homeowner looks up a quoting business and calls it directly
 "p_direct_contact_bypasser": 0.30,                # A when the quote says "look us up"
 "p_stop_random": 0.008,                           # A ordinary homeowners texting STOP mid-job
 "p_nl_optout": 0.006,                             # A "please stop texting me"
 "p_business_text_in": 0.03,                       # A inbound texts that are businesses asking to join
 "pick_base": 0.85, "pick_q_decay": 0.35,          # A pick prob = base*(1-decay^n)*speed
 "answer_prob": {"normal":.75,"screener":.10,"absent":.40,"default":.70},   # A
 "callback_prob": 0.40,                            # A
 "p_problem_quality": 0.04,                        # A
 "p_problem_if_shaded": 0.25,                      # A
 "p_spouse_gets_code": 0.30,                       # A
 "p_safe_reply": 0.90,                             # A
 "p_answer_question": 0.95,                        # A
 # ---- businesses (Part 8.2) ----
 "launch_join_share": (0.15, 0.30),                # G first-year opt-in range
 "launch_fraction": 0.5,                           # A share of year-1 joiners live at launch
 "bpersonas": {"responsive":.30,"busy":.15,"linkaverse":.10,"bypasser":.05,"noshow":.04,"liclapse":.04,
               "inslapse":.04,"multistaff":.08,"stopby":.03,"oldrecord":.06,"shader":.05,"tripgamer":.03,
               "smalldecliner":.03,"plain":.00},
 "fake_businesses": 2,                             # G per run
 "per_category_override": None,                    # crowded / thin market
 "p_after_hours": 0.25, "p_speaks_spanish": 0.20, "p_skip_tenant": 0.10, "p_max_cards": 0.30,   # A
 "p_bank_payment": 0.20,                           # A
 "p_card_added": 0.80,                             # A at second win
 "p_bus_question": 0.15,                           # A
 "p_unlinked_call": 0.5,                           # A multi-staff techs
 "p_has_pin": 0.6,                                 # A
 "p_dba_second_account": 0.03,                     # A tries a second free win under a DBA
 "background_churn_month": 0.015,                  # A
 "p_name_mismatch_city": 0.10,                     # A DBA vs legal name on city list
 # ---- vendors (Part 7) ----
 "tw": {"fail":0.015,"delay":0.01,"dup_status":0.02,"lost_status":0.005,"suspend_day":None,"suspend_days":30,
        "seg":0.0083,"carrier":0.004,"mms":0.022,"number_month":1.15,"tendlc_month":1.50,"tendlc_once":19.50,
        "call_in":0.0085,"call_out":0.014,"verify_ok":0.05,"verify_sms":0.0083,"verify_voice":0.06},   # G prices, G rates
 "stripe": {"decline":0.03,"tds":0.05,"wh_lost":0.01,"wh_late":0.02,"timeout":0.003,"timeout_processed":0.5,
            "refund_fail":0.02,"dispute":0.015,"outages":[],"random_outage_per_60d":1.0,
            "pct":0.029,"fix":0.30,"ach_pct":0.008,"ach_cap":5.0,"dispute_fee":15.0},   # G prices; A rates where brief gives none
 "ai": {"cat_err":0.04,"type_err":0.06,"miss_indirect":0.30,"injection":0.30,"photo_miss":0.10,"quote_err":0.03,
        "scrub_miss":0.05,"outages":[],"cost_request":0.02,"cost_quote":0.002,"cost_photo":0.005},       # G rates; A scrub_miss, A per-call split
 "roc": {"down":0.02,"captcha":0.05,"layout":0.01,"forced_layout_day":None},       # G
 "city_list_staleness_days": 30,                   # G up to a month
 "email_bounce": 0.03, "letter_cost": 1.0, "letter_lost": 0.05,       # A, G ($1 assumed by brief), A
 "fixed_month": 157.0,                             # G
 "start_cash": 1000.0, "stripe_reserve": 500.0,    # G
 # ---- Casey (Part 8.3) ----
 "casey": {"window_min":30, "away":None, "checklist":False, "p_fooled":0.15, "p_fooled_checklist":0.03,
           "p_approve_legit":0.95, "backup":True,
           "minutes":{"approve_signup":15,"approve_signup_checklist":25,"complaint":20,"dispute":30,"callback_call":6,
                      "alert":10,"scraper_fix":90,"spot_check":20,"ledger":20,"refund_fail":10,"shade_flag":10,
                      "pause_review":15,"optout_emergency":10,"city_mismatch":8,"letter_manual":10,"misc":5,
                      "suspension_email":2,"recruit_visit":45}},   # A minutes
 "dispute_respond_days": 7, "dispute_win": 0.6,    # A
 # ---- design switches: spec as written (False) vs proposed fixes (True) ----
 "fix": {"code_scrub_before_ai":False, "no_photos_on_cards":False, "stable_list_numbers":False,
         "pick_idempotency_key":False, "quiet_cutoff_2030":False, "spanish_optout":False, "consent_lenient":False,
         "pin_when_multi_job":False, "never_mind_cancel":False, "safety_followup":False, "window_matches_promise":False,
         "handyman_disclosure":False, "casey_checklist":False, "emergency_indirect_keywords":False,
         "permit_question":False},
 "restore": None,                                  # {"day":30,"lost_h":3,"mode":"spec"|"naive"}
 "collision": None,                                # "fwd" | "rev"
 "adversarial_mult": 1.0,
 "recurring_partial_refund": False, "p_recurring_stop_30d": 0.15,  # A
 "insurance_required": False,
 "roc_mode": "live",                               # "live" | "lists"
 "dispute_wave": None,                             # (day, n)
 "ai_outage": None, "stripe_outage": None,
 "chaos": 1.0,
 "approved_scope": False,
}

LABELS = {k: "A" for k in DEFAULT}
for k in ["requests_per_day","fees","s50","free_first_win","quote_window","value_sigma","p_zip","p_photo","p_repeat","p_email",
          "personas","p_overnight","launch_join_share","bpersonas","fake_businesses","tw","fixed_month","start_cash","stripe_reserve",
          "season"]:
    LABELS[k] = "G"

def deep_update(d, u):
    for k, v in u.items():
        if isinstance(v, dict) and isinstance(d.get(k), dict):
            deep_update(d[k], v)
        else:
            d[k] = v
    return d

def make(over=None):
    c = copy.deepcopy(DEFAULT)
    if over: deep_update(c, copy.deepcopy(over))
    return c

if __name__ == "__main__":
    json.dump({"DEFAULT":DEFAULT,"LABELS":LABELS,"FEES":FEES,"CATS":{k:list(v) for k,v in CATS.items()},
               "POOL":POOL,"CAC":CAC}, open("config.json","w"), indent=1, default=str)
