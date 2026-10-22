use super::{EstimateConfidence, ProbabilityError, UnitInterval};
use crate::core::VersionId;

#[test]
fn unit_interval_accepts_closed_boundaries() {
    assert_eq!(UnitInterval::new(0.0).unwrap().get(), 0.0);
    assert_eq!(UnitInterval::new(1.0).unwrap().get(), 1.0);
    assert_eq!(UnitInterval::new(0.42).unwrap().get(), 0.42);
}

#[test]
fn unit_interval_rejects_non_finite_and_out_of_range_values() {
    for value in [-0.01, 1.01, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(matches!(
            UnitInterval::new(value),
            Err(ProbabilityError::OutsideUnitInterval(rejected))
                if rejected.to_bits() == value.to_bits()
        ));
    }
}

#[test]
fn negative_zero_is_canonicalized() {
    let probability = UnitInterval::new(-0.0).unwrap();

    assert_eq!(probability.get().to_bits(), 0.0_f64.to_bits());
}

#[test]
fn confidence_is_unknown_without_an_explicit_calibration() {
    assert_eq!(EstimateConfidence::default(), EstimateConfidence::Unknown);
    assert_eq!(
        EstimateConfidence::Calibrated {
            probability: UnitInterval::new(0.8).unwrap(),
            calibration_version: VersionId("calibration.sensor-7-v1".into()),
        },
        EstimateConfidence::Calibrated {
            probability: UnitInterval::new(0.8).unwrap(),
            calibration_version: VersionId("calibration.sensor-7-v1".into()),
        }
    );
}
