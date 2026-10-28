use super::*;

#[test]
fn every_field_kind_has_its_own_name() {
    let mut names: Vec<&str> = FieldKind::ALL.iter().map(FieldKind::name).collect();
    assert!(names.iter().all(|name| !name.is_empty()));
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), FieldKind::ALL.len());
}

#[test]
fn a_field_kind_is_named_whatever_its_limits_are() {
    assert_eq!(
        FieldKind::Slider {
            min: 5.0,
            max: 9.0,
            step: 1.0
        }
        .name(),
        "Slider"
    );
    assert_eq!(
        FieldKind::Number {
            min: -1.0,
            max: 1.0
        }
        .name(),
        "Number"
    );
    assert_eq!(FieldKind::Choice(&["a", "b"]).name(), "Choice");
}
