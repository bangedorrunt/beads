#[test]
fn test_parse_flexible_timestamp_panic_on_multibyte() {
    let input = "+10🚀";
    let result = beads::util::time::parse_flexible_timestamp(input, "test");
    assert!(result.is_err());
}

#[test]
fn test_parse_relative_time_panic_on_multibyte() {
    let input = "+10🚀";
    let result = beads::util::time::parse_relative_time(input);
    assert!(result.is_none());
}
