# Part 3: Photo Playbook (Ask Hava)

Purpose: what photos to ask for, how to ask, and what to keep off job cards. Trade YAML files hold the detailed `photos_to_request` and `photo_cues`. This file sets the shared rules and a short starter list per trade.

Status: the rules below are Ask Hava practice **[judgment]** unless a source is cited. Safety lines cite official sources only.

## 1. General rules

**Asking**
1. Photos are always optional. "If it's easy, a photo helps businesses quote. No photo is fine too."
2. Ask for at most 2–3 photos at a time, in order of usefulness. Ask once and don't nag.
3. Give concrete framing: "a photo of the label on the side of the unit" beats "a photo of the AC."
4. One wide shot plus one close-up is the default pattern.
5. Model and serial labels are often the most useful photo for systems and appliances. A sticker photo can replace a long Q&A.
6. Offer the "Not sure / can't get to it" option every time.
7. Spanish: use the same asks in plain Spanish (trade file `triggers_es` and question text).

**Safety first (never ask for a photo that puts someone at risk)**
- No photos during an emergency (section 3 of `part1_all_reasons.md`). Safety steps come first.
- Never ask someone to climb a roof or ladder, open an electrical panel's inner cover, reach into a pool equipment pad while water is on the floor, go into an attic in summer heat, or approach a downed line. UES: keep at least 15 ft from overhead lines [S24]. CDC: never touch electrical equipment while standing in water [S31].
- Gas smell: no photos. Leave and call (UES steps) [S23].
- Garage door springs and cables: photo from a distance only; don't touch [judgment].
- Scorpions, bees, wasps: don't get close for a photo [judgment].
- Pool or spa with a tingling report: no photos until power is off and everyone is out (CPSC) [S30].

**Neutral wording (we never diagnose)**
- Good: "Thanks, I've added the photo to your request." · "This helps businesses see the setup."
- Never: "That looks like a bad capacitor," "That's definitely a leak," "That's not a big job," "That looks expensive."
- Never comment on the home's condition, cleanliness, or the homeowner's DIY work.
- If a photo shows a possible hazard (scorch marks, sparks, water near outlets, a smell they mention), share the official safety step for that sign. Example: NFPA lists "discolored or warm wall outlets" as a reason to call a qualified electrician [S29]. Don't name the cause.

**Privacy (job cards are anonymous)**
- Before a photo goes on a card, check for and leave off or crop: people and faces, kids, pets with tags, house numbers, street signs, mail and packages with labels, license plates (cars, boats, RVs, golf carts), boat registration numbers [judgment], gate, lockbox, alarm or keypad codes, Wi-Fi labels, documents, screens showing accounts, family photos, and anything identifying inside the home.
- If a photo can't be cleaned up, keep it out of the card and describe it in words instead.
- Never ask for a photo of IDs, bills with account numbers, or the front of the house from the street.
- Address and contact details never appear on cards. Businesses get the REQ-#### ID and general area only.

**Data handling**
- Keep photo-related card notes factual: "Photo shows the outdoor unit and its label" — no conclusions.
- EXIF location data should be stripped before sharing [judgment; implementation detail for the parent].

---

## 2. Per-trade starter list

Format: **photos to ask** (2–3) · *what businesses typically look for* (internal cue, never said to the homeowner as a diagnosis) · ⚠ safety note.

### Systems
- **hvac**: (1) outdoor unit label (model/serial), (2) thermostat screen, (3) indoor unit or air handler, or swamp cooler from the ground. *Cue: system type and age, heat pump vs. gas, cooler vs. refrigerated.* ⚠ No roof climbing for coolers or package units; photo from the ground.
- **plumbing**: (1) the leak or clog spot wide, (2) close-up, (3) the shutoff or the water meter area if easy. *Cue: pipe material, fixture type, access.* ⚠ Water near outlets: follow the CDC power steps first [S31].
- **water_heater**: (1) the label, (2) the whole tank and the floor around it, (3) the top connections (gas or electric, venting). *Cue: fuel, size, age, location.* ⚠ Gas smell: no photo; UES steps [S23].
- **water_treatment**: (1) softener or RO label, (2) the display screen, (3) under-sink RO setup. *Cue: brand, age, error code.*
- **electrical**: (1) the outlet, switch or fixture, (2) the panel **door** label (outer door closed or open, inner cover on), (3) the area where the new item goes. *Cue: panel brand, space, access.* ⚠ Never remove panel covers. Burning smell or sparks: NFPA guidance [S29].
- **solar**: (1) the inverter display or app screen, (2) the panels from the ground, (3) the electrical meter area. *Cue: system size, inverter type, roof type.* ⚠ No roof climbing.
- **generator_ev**: (1) the panel door label, (2) the spot for the charger or generator, (3) the gas meter or propane tank if it's a standby generator. *Cue: distance to the panel, fuel source.*

### Pools and outdoors
- **pool_spa_repair**: (1) the equipment pad wide, (2) pump or heater label, (3) the control or error screen. *Cue: equipment brand, plumbing layout.* ⚠ If water is on the pad or there's a tingling report: CPSC steps first; power off; no photos until safe [S30].
- **pool_cleaning**: (1) the pool wide, (2) the equipment pad. *Cue: size, water condition, screen enclosure, trees.*
- **landscaping_irrigation**: (1) the area wide, (2) the controller or timer, (3) the leak spot or valve box. *Cue: yard size, drip vs. spray, slope.*
- **tree_trimming**: (1) the whole tree with a house, wall or person-free reference for scale, (2) where it's near the roof, wall, pool or lines. *Cue: height, species, access.* ⚠ Near power lines: stay at least 15 ft away; UES handles line clearance [S24].
- **pavers_concrete**: (1) the area wide, (2) close-up of the crack or sinking, (3) access path (gate width). *Cue: square footage, material, access for equipment.*

### Interior and exterior
- **handyman**: one photo per list item if easy. *Cue: count of items, materials, total size (the $1,000 rule) [S4].*
- **roofing**: (1) the ceiling stain inside, (2) the roof from the ground or from a drone if they already have one. *Cue: tile, shingle or foam; slope; age.* ⚠ No roof climbing. Active leak near light fixtures: CDC power steps [S31].
- **painting**: (1) each room or wall wide, (2) problem spots (peeling, stains). *Cue: area, height, surface (stucco, block).*
- **drywall**: (1) the damage with something for scale, (2) the wall or ceiling wide. *Cue: size, texture.* Ask if there was a leak and whether it's fixed.
- **flooring**: (1) each room wide, (2) the current floor close-up, (3) transitions or damage. *Cue: area, current material, stairs.*
- **cabinets_countertops**: (1) the kitchen or bath wide, (2) the problem door or drawer. *Cue: layout, linear footage, sink and cooktop.*
- **windows_doors**: (1) the window or door from inside, (2) close-up of the glass, track or frame, (3) the label on the glass edge if visible. *Cue: size, frame material, dual pane.*
- **garage_doors**: (1) the door from inside, wide, (2) the opener motor and its label, (3) the springs **from a distance**. *Cue: size, single vs. double, opener type.* ⚠ Don't touch springs or cables [judgment].
- **appliance_repair**: (1) the model/serial label (inside the door, back, or kick plate), (2) any error display. *Cue: brand, model, gas vs. electric.* ⚠ Gas smell: UES steps [S23].

### Pests
- **pest_control**: (1) the pest or signs (droppings, mud tubes, nest) from a safe distance, (2) the area where it's seen. *Cue: pest type, location, extent.* ⚠ Bees, wasps, scorpions: don't approach. Stings: poison center [number removed, see source] [S32].

### Cleaning, junk and moving
- **house_cleaning**: optional. Room count is more useful than photos [judgment]. Remove any personal items, mail or photos from shots.
- **carpet_cleaning**: (1) rooms wide, (2) stains close-up. *Cue: square footage, stain type.*
- **window_cleaning**: (1) the house sides showing windows (no house numbers), (2) high or hard-to-reach glass. *Cue: count, stories, screens.*
- **gutter_solar_cleaning**: (1) the panels or gutters from the ground. *Cue: count, roof height.* ⚠ No climbing.
- **junk_removal**: (1) the pile or items wide. *Cue: volume (truck loads), heavy items (hot tub, piano).* Crop out documents and anything with a name.
- **moving**: (1) the largest items, (2) stairs or tight spots. *Cue: room count, heavy items.* Don't photograph valuables or documents.

### Vehicles and water
- **boat_dock**: (1) the boat or lift wide (crop out registration numbers and the boat name if it's unique [judgment]), (2) the engine or lift motor label. *Cue: boat type and length, engine make, lift type.* ⚠ Dock shock risk: no one in the water near the dock while power is on (see CPSC/NFPA E5 in part 1) [S30].
- **rv_services**: (1) the problem area, (2) the RV data plate or label (crop out the VIN [judgment]). *Cue: RV type, length.* Crop the plate.
- **golf_cart**: (1) the cart wide (crop the plate), (2) the battery compartment or charger display. *Cue: make, gas vs. electric, battery type.*

### Home safety and inspection
- **home_inspection**: none needed. Ask for square footage, pool yes/no, and the age of the home instead [judgment].
- **locksmith**: (1) the lock face, (2) the door edge or the lock brand stamp. Never photograph keys up close (they can be copied from photos [judgment]) or keypad codes.
- **security_alarm**: (1) the alarm panel or keypad (code area hidden), (2) the existing camera or doorbell. *Cue: brand, wired vs. wireless.* Never capture codes.
- **septic**: (1) the tank lid or risers if visible, (2) the alarm light or control box. *Cue: aerobic vs. conventional, access.* ⚠ Don't open tank lids [judgment].
