# Flood segmentation

A small U-Net that marks water in Sentinel-2 images, trained on hand-labelled
flood scenes and run inside Lifeline through ONNX. This folder holds what is
needed to reproduce it: download, training, evaluation, export, and a check
on the imagery the app actually uses.

## Data

[Sen1Floods11](https://github.com/cloudtostreet/Sen1Floods11) (Bonafilia
et al., CVPR Workshops 2020), the hand-labelled part: 446 chips of
512 × 512 pixels at 10 m from 11 flood events on five continents, each with
a Sentinel-2 L1C image and a per-pixel label (water, not water, not labelled).
The authors' splits are used unchanged: 252 chips for training, 89 for
validation, 90 for testing, and 15 from a Bolivian flood held out entirely as
an unseen region.

The dataset has no explicit licence (the authors ask to be cited), so the
images, labels and trained weights are not committed or distributed here.

## Model

| | |
| --- | --- |
| Input | Six bands Lifeline already fetches (B02, B03, B04, B08, B11, B12) as reflectance, plus MNDWI and NDWI computed from them, standardised with training-set statistics (`band_stats.json`) |
| Network | U-Net, four scales, 16 to 128 channels, batch norm, 0.48 M parameters, 1.9 MB as ONNX |
| Loss | Cross-entropy plus soft Dice on the water class; unlabelled pixels ignored |
| Augmentation | Random 256-pixel crops, quarter-turn rotations and flips; per-band gain (±10 %) and offset (±0.015) so the network does not depend on one processing level |
| Training | AdamW, one-cycle learning rate, 30 epochs on a CPU; the checkpoint with the best validation IoU is kept |
| Resolution | 10 m by default; `--scales 1,2` also trains on chips block-averaged to 20 m, the resolution the app runs the network at |

The exported graph includes the band arithmetic and standardisation, so the
app passes plain reflectance in and gets a water probability out.

## Results

Two networks were trained with the same settings: one on 10 m chips only,
and one on 10 m and 20 m chips (`--scales 1,2`). Water IoU on the held-out
splits, pixels pooled:

| Detector | Test, 10 m | Bolivia, 10 m | Test, 20 m | Bolivia, 20 m |
| --- | ---: | ---: | ---: | ---: |
| MNDWI > 0.05 (shipped index) | 0.728 | 0.611 | 0.715 | 0.604 |
| MNDWI > 0.15 (tuned on train + valid) | 0.787 | 0.703 | 0.791 | 0.701 |
| U-Net, 10 m training | 0.863 | 0.820 | 0.849 | 0.808 |
| U-Net, 10 and 20 m training | 0.858 | 0.811 | 0.846 | 0.793 |

Precision and recall on the test split at 10 m:

| Detector | Precision | Recall | F1 | Chip IoU |
| --- | ---: | ---: | ---: | ---: |
| MNDWI > 0.05 | 0.758 | 0.948 | 0.842 | 0.510 |
| MNDWI > 0.15 | 0.867 | 0.894 | 0.881 | 0.514 |
| U-Net, 10 m training | 0.944 | 0.910 | 0.927 | 0.630 |
| U-Net, 10 and 20 m training | 0.933 | 0.914 | 0.924 | 0.630 |

The two networks are within noise of each other on the labelled data. The
multi-resolution one is the one to deploy, because it holds up better on
the app's own imagery (below).

IoU is the intersection over union of the water class, with every pixel of
a split pooled. Chip IoU is the mean of the per-chip values, so a chip with a
little water counts as much as a heavily flooded one; chips where neither
the label nor the prediction has any water are left out. The tuned index uses
the threshold that does best on the training and validation splits, so the
network is compared with the strongest version of the rule rather than a
strawman.

### On the imagery the app uses

The training chips are L1C (top of atmosphere); the app reads L2A (surface
reflectance) from Planetary Computer. There are no hand labels for those
scenes, so `field_check.py` compares each detector with ESA WorldCover
permanent water on clear dry-season scenes at six sites. WorldCover is a
weak reference (rivers move, wetlands are seasonal), so this checks for a
domain gap rather than ranking the detectors.

| Site | Scene | WorldCover water | Index: found | Index: land flagged | U-Net: found | U-Net: land flagged |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| Indus near Dadu, Pakistan | 2021-12-17 | 1.8 % | 54.7 % | 0.98 % | 42.7 % | 0.48 % |
| Sylhet haor, Bangladesh | 2023-01-21 | 4.3 % | 67.1 % | 8.44 % | 68.4 % | 4.98 % |
| Buzi and Pungwe, Mozambique | 2023-07-19 | 2.6 % | 93.9 % | 0.01 % | 94.3 % | 0.02 % |
| Tonle Sap, Cambodia | 2024-01-30 | 12.0 % | 98.8 % | 0.68 % | 98.5 % | 0.56 % |
| Kahramanmaraş, Türkiye | 2023-08-18 | 0.0 % | 16.8 % | 0.72 % | 2.7 % | 0.02 % |
| Po near Piacenza, Italy | 2023-08-21 | 6.7 % | 87.0 % | 0.15 % | 86.7 % | 0.11 % |
| All sites | | | 86.4 % | 1.86 % | 85.5 % | 1.03 % |

The multi-resolution U-Net finds as much known water as the index on L2A
imagery and flags about half as much dry land. The network trained at 10 m
only was more conservative here (82.6 % found, 0.73 % flagged). Both miss
part of the narrow, silty Indus channels; land flagged at Sylhet is largely
seasonal wetland that WorldCover does not count as permanent.

## Reproduce

```bash
python -m venv .venv && . .venv/bin/activate
pip install -r ml/requirements.txt --extra-index-url https://download.pytorch.org/whl/cpu
python ml/download.py          # about 1 GB into ml/data/
python ml/train.py --scales 1,2 # about an hour on four CPU cores
python ml/evaluate.py          # the tables above, at 10 m
python ml/evaluate.py --scale 2 # and at 20 m
python ml/export_onnx.py       # ml/runs/water-unet.onnx, checked against PyTorch
python ml/field_check.py       # the L2A check
```

Then copy `ml/runs/water-unet.onnx` to Lifeline's model folder
(`~/.local/share/lifeline/models/` on Linux) or point `LIFELINE_WATER_MODEL`
at it, and the flood check offers "Trained U-Net" as a detector. The CLI
takes the file directly: `lifeline flood -s area.json --model
ml/runs/water-unet.onnx`.

## Files

| File | Purpose |
| --- | --- |
| `download.py` | Fetches the hand-labelled chips and the official splits |
| `common.py` | Band selection, features, metrics |
| `model.py` | The U-Net |
| `train.py` | Training loop |
| `evaluate.py` | Index and network on the test and Bolivia splits |
| `export_onnx.py` | ONNX export with preprocessing, verified against PyTorch |
| `field_check.py` | Comparison on Planetary Computer L2A scenes |
| `band_stats.json` | Channel means and standard deviations from the training split |
