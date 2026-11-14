use super::*;

#[test]
fn a_resting_job_is_solid_and_a_busy_one_pulses_between_55_and_100() {
    assert_eq!(pulse(false, 0.3), 1.0);
    assert!((pulse(true, 0.0) - 0.55).abs() < 1e-6);
    assert!((pulse(true, 0.5) - 1.0).abs() < 1e-6);
    for step in 0..20 {
        let alpha = pulse(true, step as f32 / 20.0);
        assert!((0.55..=1.0).contains(&alpha));
    }
}
