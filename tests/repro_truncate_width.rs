use beads::format::truncate_title;

#[test]
fn test_truncate_title_with_wide_characters() {

    let title = "🚀🚀🚀";
    let _truncated = truncate_title(title, 4);

    let title_mixed = "あaaaaa";
    let truncated_mixed = truncate_title(title_mixed, 6);

    assert_eq!(
        truncated_mixed, "あa...",
        "Logic should correctly truncate to visual width 6"
    );

    let title_emoji = "🚀🚀🚀";
    let truncated_emoji = truncate_title(title_emoji, 4);
    assert_eq!(
        truncated_emoji, "...",
        "Logic should handle wide emoji correctly"
    );
}
