//! 简单 ID 工具。

/// 生成 UUID v4 格式（36 位，含连字符）的随机字符串。
pub fn uuid_v4() -> String {
    use rand::RngExt;
    let mut rng = rand::rng();
    let mut bytes = [0u8; 16];
    for byte in &mut bytes {
        *byte = rng.random_range(0..=255u8);
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex = hex::encode(bytes);
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn uuid_has_expected_shape() {
        let value = super::uuid_v4();
        assert_eq!(value.len(), 36);
        assert_eq!(value.as_bytes()[8], b'-');
        assert_eq!(&value[14..15], "4");
    }

    #[test]
    fn uuids_are_unique() {
        let mut ids = std::collections::HashSet::new();
        for _ in 0..128 {
            assert!(ids.insert(super::uuid_v4()));
        }
    }
}
