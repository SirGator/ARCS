use arcs_normalize_number::{NumberInput, normalize};

#[test]
fn normalization_supports_different_units_and_parameter_sets() {
    for (value, scale, offset, unit, expected) in [
        (310.0, 0.1, 0.0, "Cel", 31.0),
        (10.0, 2.0, -1.0, "m/s", 19.0),
        (-0.0, 1.0, 0.0, "V", 0.0),
    ] {
        let output = normalize(NumberInput {
            value,
            scale,
            offset,
            unit: unit.into(),
        })
        .unwrap();
        assert_eq!(output.value, expected);
        assert_eq!(output.unit, unit);
        if expected == 0.0 {
            assert!(output.value.is_sign_positive());
        }
    }
}

#[test]
fn overflow_nonfinite_numbers_and_ambiguous_units_are_rejected() {
    assert!(
        normalize(NumberInput {
            value: 1e308,
            scale: 1e308,
            offset: 0.0,
            unit: "V".into()
        })
        .is_err()
    );
    for unit in ["", " ", "V\n"] {
        assert!(
            normalize(NumberInput {
                value: 1.0,
                scale: 1.0,
                offset: 0.0,
                unit: unit.into()
            })
            .is_err()
        );
    }
    assert!(
        normalize(NumberInput {
            value: f64::NAN,
            scale: 1.0,
            offset: 0.0,
            unit: "V".into()
        })
        .is_err()
    );
}
