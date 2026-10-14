"""Fetch the hand-labelled part of Sen1Floods11 (Sentinel-2 chips and labels).

Bonafilia et al., "Sen1Floods11: a georeferenced dataset to train and test
deep learning flood algorithms for Sentinel-1", CVPR Workshops 2020.
https://github.com/cloudtostreet/Sen1Floods11

446 chips of 512 x 512 px at 10 m, 13 Sentinel-2 L1C bands, about 1 GB.
Files land in ml/data/, which is not committed.
"""

import argparse
import concurrent.futures as cf
import csv
import io
import pathlib
import urllib.request

BASE = "https://storage.googleapis.com/sen1floods11/v1.1"
SPLITS = ["train", "valid", "test", "bolivia"]


def get(url: str) -> bytes:
    req = urllib.request.Request(url, headers={"User-Agent": "lifeline-ml"})
    with urllib.request.urlopen(req, timeout=120) as r:
        return r.read()


def chips(split: str) -> list[str]:
    url = f"{BASE}/splits/flood_handlabeled/flood_{split}_data.csv"
    rows = csv.reader(io.StringIO(get(url).decode()))
    return [r[0].replace("_S1Hand.tif", "") for r in rows if r]


def fetch(url: str, dest: pathlib.Path) -> None:
    if dest.exists() and dest.stat().st_size > 0:
        return
    tmp = dest.with_suffix(".part")
    tmp.write_bytes(get(url))
    tmp.rename(dest)


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--out", default=pathlib.Path(__file__).parent / "data", type=pathlib.Path)
    ap.add_argument("--workers", default=8, type=int)
    args = ap.parse_args()

    (args.out / "S2Hand").mkdir(parents=True, exist_ok=True)
    (args.out / "LabelHand").mkdir(parents=True, exist_ok=True)
    jobs = []
    for split in SPLITS:
        ids = chips(split)
        (args.out / f"{split}.txt").write_text("\n".join(ids) + "\n")
        print(f"{split:8s} {len(ids)} chips")
        for cid in ids:
            for kind in ("S2Hand", "LabelHand"):
                name = f"{cid}_{kind}.tif"
                url = f"{BASE}/data/flood_events/HandLabeled/{kind}/{name}"
                jobs.append((url, args.out / kind / name))

    with cf.ThreadPoolExecutor(args.workers) as pool:
        futures = [pool.submit(fetch, u, d) for u, d in jobs]
        for i, f in enumerate(cf.as_completed(futures), 1):
            f.result()
            if i % 100 == 0 or i == len(jobs):
                print(f"{i}/{len(jobs)} files")


if __name__ == "__main__":
    main()
