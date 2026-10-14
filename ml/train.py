"""Train the water segmentation U-Net on Sen1Floods11 hand labels (CPU is fine).

    python ml/train.py --epochs 30

Keeps the checkpoint with the best validation IoU in ml/runs/.
"""

import argparse
import json
import pathlib
import time

import numpy as np
import torch
import torch.nn.functional as F

from common import IGNORE, STATS, Confusion, compute_stats, downsample, features, load_chip, load_stats, split_ids
from model import UNet

RUNS = pathlib.Path(__file__).parent / "runs"


def load_split(split: str, stats: dict, factor: int = 1) -> tuple[np.ndarray, np.ndarray]:
    xs, ys = [], []
    for cid in split_ids(split):
        refl, y = downsample(*load_chip(cid), factor)
        xs.append(features(refl, stats).astype(np.float16))
        ys.append(y)
    return np.stack(xs), np.stack(ys)


def jitter_reflectance(x: torch.Tensor, stats: dict, rng: np.random.Generator) -> torch.Tensor:
    """Small per-band gain and offset on the six reflectance channels so the
    model does not lean on one processing level (training chips are L1C,
    Lifeline feeds it L2A)."""
    std = torch.tensor(stats["std"][:6]).view(1, 6, 1, 1)
    mean = torch.tensor(stats["mean"][:6]).view(1, 6, 1, 1)
    b = x.shape[0]
    gain = torch.from_numpy(rng.uniform(0.9, 1.1, (b, 6, 1, 1)).astype(np.float32))
    offset = torch.from_numpy(rng.uniform(-0.015, 0.015, (b, 6, 1, 1)).astype(np.float32))
    refl = x[:, :6] * std + mean
    refl = (refl * gain + offset).clamp(min=0)
    g, n, s = refl[:, 1], refl[:, 3], refl[:, 4]
    mndwi = (g - s) / (g + s).clamp(min=1e-4)
    ndwi = (g - n) / (g + n).clamp(min=1e-4)
    out = torch.cat([refl, mndwi[:, None], ndwi[:, None]], dim=1)
    m = torch.tensor(stats["mean"]).view(1, -1, 1, 1)
    sd = torch.tensor(stats["std"]).view(1, -1, 1, 1)
    return (out - m) / sd


def batches(sets: list[tuple[np.ndarray, np.ndarray]], crop: int, batch: int, rng: np.random.Generator):
    """Two crops per training chip per epoch; with several resolutions each
    crop comes from one of them at random."""
    n = len(sets[0][0])
    order = np.concatenate([rng.permutation(n), rng.permutation(n)])
    for start in range(0, len(order) - batch + 1, batch):
        xb, yb = [], []
        for i in order[start : start + batch]:
            x, y = sets[int(rng.integers(len(sets)))]
            r, c = rng.integers(0, x.shape[2] - crop + 1, 2)
            xi = x[i, :, r : r + crop, c : c + crop]
            yi = y[i, r : r + crop, c : c + crop]
            k = int(rng.integers(4))
            xi, yi = np.rot90(xi, k, (1, 2)), np.rot90(yi, k)
            if rng.random() < 0.5:
                xi, yi = xi[:, :, ::-1], yi[:, ::-1]
            xb.append(np.ascontiguousarray(xi, dtype=np.float32))
            yb.append(np.ascontiguousarray(yi))
        yield torch.from_numpy(np.stack(xb)), torch.from_numpy(np.stack(yb)).long()


def loss_fn(logits: torch.Tensor, y: torch.Tensor) -> torch.Tensor:
    ce = F.cross_entropy(logits, y, ignore_index=IGNORE)
    valid = (y != IGNORE).float()
    p = logits.softmax(1)[:, 1] * valid
    t = (y == 1).float() * valid
    dice = 1 - (2 * (p * t).sum() + 1) / (p.sum() + t.sum() + 1)
    return ce + dice


@torch.no_grad()
def evaluate(model: UNet, x: np.ndarray, y: np.ndarray) -> dict:
    model.eval()
    conf = Confusion()
    for i in range(0, len(x), 4):
        logits = model(torch.from_numpy(x[i : i + 4].astype(np.float32)))
        pred = logits.argmax(1).numpy()
        for p, t in zip(pred, y[i : i + 4]):
            conf.add(p, t)
    model.train()
    return conf.summary()


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--epochs", type=int, default=30)
    ap.add_argument("--batch", type=int, default=16)
    ap.add_argument("--crop", type=int, default=256)
    ap.add_argument("--lr", type=float, default=2e-3)
    ap.add_argument("--seed", type=int, default=7)
    ap.add_argument("--scales", default="1", help="comma-separated resolution factors, e.g. 1,2 for 10 and 20 m")
    ap.add_argument("--out", default=RUNS, type=pathlib.Path)
    args = ap.parse_args()
    factors = [int(f) for f in args.scales.split(",")]

    torch.manual_seed(args.seed)
    rng = np.random.default_rng(args.seed)
    if not STATS.exists():
        STATS.write_text(json.dumps(compute_stats(split_ids("train")), indent=2) + "\n")
    stats = load_stats()

    t0 = time.time()
    train = [load_split("train", stats, f) for f in factors]
    valid = [load_split("valid", stats, f) for f in factors]
    print(f"loaded {len(train[0][0])} train, {len(valid[0][0])} valid chips at scales {factors} in {time.time() - t0:.0f} s", flush=True)

    model = UNet()
    opt = torch.optim.AdamW(model.parameters(), lr=args.lr, weight_decay=1e-4)
    steps = args.epochs * (2 * len(train[0][0]) // args.batch)
    sched = torch.optim.lr_scheduler.OneCycleLR(opt, max_lr=args.lr, total_steps=steps, pct_start=0.1)

    args.out.mkdir(parents=True, exist_ok=True)
    best, log = -1.0, []
    for epoch in range(1, args.epochs + 1):
        t0, total, n = time.time(), 0.0, 0
        for xb, yb in batches(train, args.crop, args.batch, rng):
            xb = jitter_reflectance(xb, stats, rng)
            loss = loss_fn(model(xb), yb)
            opt.zero_grad()
            loss.backward()
            opt.step()
            sched.step()
            total, n = total + loss.item(), n + 1
        # select on the mean IoU over the resolutions trained for
        vals = [evaluate(model, x, y) for x, y in valid]
        val = {k: float(np.mean([v[k] for v in vals])) for k in vals[0]}
        log.append({"epoch": epoch, "loss": total / n, **{f"val_{k}": v for k, v in val.items()}})
        mark = ""
        if val["iou"] > best:
            best = val["iou"]
            torch.save({"state": model.state_dict(), "epoch": epoch, "stats": stats, "scales": factors}, args.out / "best.pt")
            mark = "  *"
        print(
            f"epoch {epoch:2d}  loss {total / n:.3f}  val IoU {val['iou']:.3f}  "
            f"P {val['precision']:.3f}  R {val['recall']:.3f}  {time.time() - t0:.0f} s{mark}",
            flush=True,
        )
        (args.out / "log.json").write_text(json.dumps(log, indent=1))


if __name__ == "__main__":
    main()
