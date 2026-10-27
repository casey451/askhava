"""Re-run one scenario on seeds 1-50 (used after a checker correction): python3 rerun_one.py S12_db_restore"""
import sys, os, json
from multiprocessing import Pool
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from run_all import job, SEEDS, OUT
from scenarios import SCEN
if __name__ == "__main__":
    n = sys.argv[1]
    with Pool(8) as p: L = p.map(job, [(n, SCEN[n], s, True) for s in SEEDS])
    L.sort(key=lambda m: m["seed"]); json.dump(L, open(os.path.join(OUT, f"raw_{n}.json"), "w"), default=str)
    print(n, "errors", sum(1 for m in L if "error" in m))
