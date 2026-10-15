//! The ONNX runner, with a formula standing in for the network.

use lifeline_geodata::{WaterModel, MODEL_BANDS};
use std::path::Path;

#[test]
fn tiled_inference_matches_the_formula_everywhere() {
    let model = WaterModel::load(Path::new("tests/fixtures/mndwi-sigmoid.onnx")).unwrap();
    // not a multiple of the tile size, and wider than tall
    let (rows, cols) = (300, 530);
    let plane = rows * cols;
    let mut refl = vec![0.0f32; MODEL_BANDS.len() * plane];
    for r in 0..rows {
        for c in 0..cols {
            let p = r * cols + c;
            // a smooth green/SWIR pattern that varies across tile seams
            refl[plane + p] = 0.05 + 0.04 * ((r as f32) * 0.05).sin();
            refl[4 * plane + p] = 0.05 + 0.04 * ((c as f32) * 0.03).cos();
            refl[p] = 0.9; // other bands must be ignored by this model
        }
    }
    let prob = model.predict(&refl, rows, cols).unwrap();
    assert_eq!(prob.len(), plane);
    let mut worst = 0.0f32;
    for p in 0..plane {
        let (g, s) = (refl[plane + p], refl[4 * plane + p]);
        let expect = 1.0 / (1.0 + (-20.0 * (g - s) / (g + s)).exp());
        worst = worst.max((prob[p] - expect).abs());
    }
    assert!(worst < 1e-5, "max error {worst}");
}

#[test]
fn rejects_mismatched_input() {
    let model = WaterModel::load(Path::new("tests/fixtures/mndwi-sigmoid.onnx")).unwrap();
    assert!(model.predict(&[0.0; 10], 4, 4).is_err());
    assert!(WaterModel::load(Path::new("tests/fixtures/README.md")).is_err());
}

#[test]
fn rejects_a_model_whose_output_is_not_one_value_per_pixel() {
    // ReduceMean over bands, then a 2x2 max pool: half-resolution output
    let model = WaterModel::load(Path::new("tests/fixtures/half-resolution.onnx")).unwrap();
    let refl = vec![0.1f32; MODEL_BANDS.len() * 40 * 40];
    assert!(model.predict(&refl, 40, 40).is_err());
}
