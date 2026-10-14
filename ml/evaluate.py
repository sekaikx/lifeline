"""Score water detectors on the Sen1Floods11 held-out splits.

    python ml/evaluate.py            # MNDWI baselines and the trained U-Net
    python ml/evaluate.py --json     # machine-readable

The MNDWI threshold Lifeline ships with (0.05) is scored as is; a second
baseline uses the threshold that maximises IoU on train + valid, so the
comparison with the network is not against a strawman.
"""

import argparse
import json
import pathlib

import numpy as np

from common import IGNORE, Confusion, downsample, features, load_chip, mndwi, split_ids

RUNS = pathlib.Path(__file__).parent / "runs"
SPLITS = ["test", "bolivia"]


def tune_threshold(ids: list[str]) -> float:
    grid = np.round(np.arange(-0.30, 0.41, 0.01), 2)
    tp = np.zeros(len(grid))
    fp = np.zeros(len(grid))
    fn = np.zeros(len(grid))
    for cid in ids:
        refl, y = load_chip(cid)
        m = y != IGNORE
        v, t = mndwi(refl)[m], y[m] == 1
        for k, th in enumerate(grid):
            p = v > th
            tp[k] += (p & t).sum()
            fp[k] += (p & ~t).sum()
            fn[k] += (~p & t).sum()
    return float(grid[np.argmax(tp / (tp + fp + fn))])


def unet_predictor(path: pathlib.Path):
    import torch

    from model import UNet

    ckpt = torch.load(path, weights_only=False)
    net = UNet()
    net.load_state_dict(ckpt["state"])
    net.eval()

    def predict(refl: np.ndarray) -> np.ndarray:
        with torch.no_grad():
            x = torch.from_numpy(features(refl, ckpt["stats"]))[None]
            return net(x).argmax(1)[0].numpy()

    return predict, ckpt["epoch"]


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--json", action="store_true")
    ap.add_argument("--baselines", action="store_true", help="skip the network")
    ap.add_argument("--ckpt", default=[RUNS / "best.pt"], type=pathlib.Path, nargs="+")
    ap.add_argument("--scale", default=1, type=int, help="evaluate at 10·scale m (block-averaged chips)")
    args = ap.parse_args()

    tuned = tune_threshold(split_ids("train") + split_ids("valid"))
    detectors = {
        "MNDWI > 0.05 (shipped)": lambda r: mndwi(r) > 0.05,
        f"MNDWI > {tuned:.2f} (tuned on train+valid)": lambda r, t=tuned: mndwi(r) > t,
    }
    for ckpt in [] if args.baselines else [c for c in args.ckpt if c.exists()]:
        predict, epoch = unet_predictor(ckpt)
        detectors[f"U-Net {ckpt.parent.name} (epoch {epoch})"] = predict

    results = {}
    for split in SPLITS:
        ids = split_ids(split)
        confs = {name: Confusion() for name in detectors}
        for cid in ids:
            refl, y = downsample(*load_chip(cid), args.scale)
            for name, det in detectors.items():
                confs[name].add(det(refl), y)
        results[split] = {name: c.summary() for name, c in confs.items()}

    if args.json:
        print(json.dumps(results, indent=2))
        return
    for split, rows in results.items():
        print(f"\n{split} ({len(split_ids(split))} chips at {10 * args.scale} m)")
        print(f"  {'detector':42s} {'IoU':>6s} {'chip IoU':>9s} {'prec':>6s} {'recall':>7s} {'F1':>6s}")
        for name, s in rows.items():
            print(
                f"  {name:42s} {s['iou']:6.3f} {s['mean_chip_iou']:9.3f} "
                f"{s['precision']:6.3f} {s['recall']:7.3f} {s['f1']:6.3f}"
            )


if __name__ == "__main__":
    main()
