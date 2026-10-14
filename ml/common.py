"""Data loading and features shared by training, evaluation and export."""

import json
import pathlib

import numpy as np
import tifffile

DATA = pathlib.Path(__file__).parent / "data"
STATS = pathlib.Path(__file__).parent / "band_stats.json"

# Sen1Floods11 S2 chips carry all 13 L1C bands in this order:
# B1 B2 B3 B4 B5 B6 B7 B8 B8A B9 B10 B11 B12
L1C_ORDER = ["B01", "B02", "B03", "B04", "B05", "B06", "B07", "B08", "B8A", "B09", "B10", "B11", "B12"]
# The model reads the six bands Lifeline already fetches from Sentinel-2 L2A.
BANDS = ["B02", "B03", "B04", "B08", "B11", "B12"]
BAND_INDEX = [L1C_ORDER.index(b) for b in BANDS]
# then two derived channels: MNDWI (green/SWIR1) and NDWI (green/NIR)
CHANNELS = BANDS + ["MNDWI", "NDWI"]
IGNORE = 255


def split_ids(split: str) -> list[str]:
    return (DATA / f"{split}.txt").read_text().split()


def load_chip(cid: str) -> tuple[np.ndarray, np.ndarray]:
    """Reflectance (6, H, W) float32 in [0, ~1] and labels (H, W) uint8
    with 0 dry, 1 water, IGNORE unlabelled."""
    s2 = tifffile.imread(DATA / "S2Hand" / f"{cid}_S2Hand.tif")
    refl = s2[BAND_INDEX].astype(np.float32) / 10_000.0
    lab = tifffile.imread(DATA / "LabelHand" / f"{cid}_LabelHand.tif")
    y = np.where(lab < 0, IGNORE, lab).astype(np.uint8)
    # pixels with no imagery cannot be classified from it
    y[refl.max(axis=0) <= 0] = IGNORE
    return refl, y


def downsample(refl: np.ndarray, y: np.ndarray, f: int) -> tuple[np.ndarray, np.ndarray]:
    """Average reflectance over f x f blocks (10 m to 10·f m) and label each
    block by majority of its labelled pixels; blocks mostly unlabelled are
    ignored."""
    if f == 1:
        return refl, y
    b, h, w = refl.shape
    h, w = h // f * f, w // f * f
    r = refl[:, :h, :w].reshape(b, h // f, f, w // f, f).mean(axis=(2, 4))
    blocks = y[:h, :w].reshape(h // f, f, w // f, f)
    wet = (blocks == 1).sum(axis=(1, 3))
    dry = (blocks == 0).sum(axis=(1, 3))
    out = np.where(wet > dry, 1, 0).astype(np.uint8)
    out[2 * (wet + dry) < f * f] = IGNORE
    return r.astype(np.float32), out


def index(a: np.ndarray, b: np.ndarray) -> np.ndarray:
    return (a - b) / np.maximum(a + b, 1e-4)


def mndwi(refl: np.ndarray) -> np.ndarray:
    return index(refl[1], refl[4])


def features(refl: np.ndarray, stats: dict) -> np.ndarray:
    """Standardised model input (8, H, W)."""
    x = np.concatenate([refl, mndwi(refl)[None], index(refl[1], refl[3])[None]])
    mean = np.asarray(stats["mean"], np.float32)[:, None, None]
    std = np.asarray(stats["std"], np.float32)[:, None, None]
    return ((x - mean) / std).astype(np.float32)


def raw_channels(refl: np.ndarray) -> np.ndarray:
    return np.concatenate([refl, mndwi(refl)[None], index(refl[1], refl[3])[None]])


def compute_stats(ids: list[str]) -> dict:
    total = np.zeros(len(CHANNELS))
    sq = np.zeros(len(CHANNELS))
    n = 0
    for cid in ids:
        refl, _ = load_chip(cid)
        x = raw_channels(refl)
        valid = refl.max(axis=0) > 0
        v = x[:, valid].astype(np.float64)
        total += v.sum(axis=1)
        sq += (v * v).sum(axis=1)
        n += v.shape[1]
    mean = total / n
    std = np.sqrt(sq / n - mean * mean)
    return {"channels": CHANNELS, "mean": mean.round(5).tolist(), "std": std.round(5).tolist()}


def load_stats() -> dict:
    return json.loads(STATS.read_text())


class Confusion:
    """Accumulates water-class counts over pixels with a label."""

    def __init__(self) -> None:
        self.tp = self.fp = self.fn = self.tn = 0
        self.chip_iou: list[float] = []

    def add(self, pred: np.ndarray, y: np.ndarray) -> None:
        m = y != IGNORE
        p, t = pred[m].astype(bool), y[m].astype(bool)
        tp = int((p & t).sum())
        fp = int((p & ~t).sum())
        fn = int((~p & t).sum())
        self.tp += tp
        self.fp += fp
        self.fn += fn
        self.tn += int((~p & ~t).sum())
        if tp + fp + fn > 0:
            self.chip_iou.append(tp / (tp + fp + fn))

    def summary(self) -> dict:
        tp, fp, fn, tn = self.tp, self.fp, self.fn, self.tn
        prec = tp / max(tp + fp, 1)
        rec = tp / max(tp + fn, 1)
        return {
            "iou": tp / max(tp + fp + fn, 1),
            "mean_chip_iou": float(np.mean(self.chip_iou)) if self.chip_iou else 0.0,
            "precision": prec,
            "recall": rec,
            "f1": 2 * prec * rec / max(prec + rec, 1e-9),
            "accuracy": (tp + tn) / max(tp + fp + fn + tn, 1),
            "pixels": tp + fp + fn + tn,
        }
