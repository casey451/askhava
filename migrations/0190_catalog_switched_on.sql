-- catalog (T486): when each category was last switched on, for the free first win's "90 days after a
-- category they signed up for is switched on" (FEE-010, LIC-002; D-019, D-195, D-3779).
-- catalog.category.switched_on_at is set to now() by the only two writers that switch a category
-- on: the loader's insert of a new category the file has on, and the owner's off-to-on switch
-- (app/catalog/src/service.rs). Switching off leaves it, so a later switch-off never undoes a
-- business's window. Backfill: every category already on reads 1970-01-01, "on before launch"
-- (covered by the launch window, D-019); off categories stay null. No real business exists before
-- Gate C, so this gives the same windows as the latest ops audit row, which a catalog migration
-- may not name (TD-SEC-35, D-1632). The check is added NOT VALID, then validated after the
-- backfill, so no write waits on the scan (postgres brain P2).

ALTER TABLE catalog.category ADD COLUMN switched_on_at timestamptz;

UPDATE catalog.category SET switched_on_at = '1970-01-01 00:00:00+00'
  WHERE enabled AND switched_on_at IS NULL;

ALTER TABLE catalog.category ADD CONSTRAINT category_switched_on_check
  CHECK (NOT enabled OR switched_on_at IS NOT NULL) NOT VALID;
ALTER TABLE catalog.category VALIDATE CONSTRAINT category_switched_on_check;
