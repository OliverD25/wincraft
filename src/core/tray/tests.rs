use super::copy_into;

fn text_of(buffer: &[u16]) -> String {
    let end = buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end])
}

#[test]
fn a_short_text_is_copied_whole_and_ends_with_a_zero() {
    let mut buffer = [0xFFFFu16; 8];
    copy_into(&mut buffer, "Hi");
    assert_eq!(&buffer[..3], &[b'H' as u16, b'i' as u16, 0]);
}

#[test]
fn a_long_text_is_cut_to_fit_and_still_ends_with_a_zero() {
    let mut buffer = [0xFFFFu16; 4];
    copy_into(&mut buffer, "WinCraft");
    assert_eq!(buffer[3], 0);
    assert_eq!(text_of(&buffer), "Win");
}

#[test]
fn an_empty_buffer_is_left_alone() {
    let mut buffer: [u16; 0] = [];
    copy_into(&mut buffer, "anything");
}
