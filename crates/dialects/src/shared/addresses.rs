use crate::shared::errors::FileSourceMap;
use lazy_static::lazy_static;
use move_core_types::account_address::AccountAddress;
use regex::Regex;

lazy_static! {
    static ref LIBRA_16_BYTES_REGEX: Regex = Regex::new(r"(0x[0-9a-f]{1,32})[^0-9a-f]").unwrap();
}

pub fn replace_16_bytes_libra(source: &str, file_source_map: &mut FileSourceMap) -> String {
    let mut transformed_source = source.to_string();

    while let Some(mat) = LIBRA_16_BYTES_REGEX.captures(&transformed_source.clone()) {
        let item = mat.get(1).unwrap();

        let orig_address = item.as_str();
        let account_address = AccountAddress::from_hex_literal(orig_address).unwrap();
        let repl_address = format!("0x00000000{}", account_address);

        file_source_map.insert_address_layer(
            item.end(),
            orig_address.to_string(),
            repl_address.clone(),
        );
        transformed_source.replace_range(item.range(), &repl_address);
    }
    transformed_source
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_libra_16_byte_address() {
        let source = "use 0x00000000000000001111111111111111;";
        let replaced = replace_16_bytes_libra(source, &mut FileSourceMap::default());
        assert_eq!(replaced, "use 0x0000000000000000000000001111111111111111;");
    }

    #[test]
    fn replace_multiple_addresses() {
        let source =
            "use 0x00000000000000001111111111111111; \n use 0x00000000000000001111111111111112;";
        let replaced = replace_16_bytes_libra(source, &mut FileSourceMap::default());
        assert_eq!(replaced, "use 0x0000000000000000000000001111111111111111; \n use 0x0000000000000000000000001111111111111112;");
    }

    #[test]
    fn dont_replace_20_bytes_address() {
        let source = "use 0x0000000000000000000000001111111111111111;";
        let replaced = replace_16_bytes_libra(source, &mut FileSourceMap::default());
        assert_eq!(replaced, "use 0x0000000000000000000000001111111111111111;");
    }

    #[test]
    fn replace_minified_addresses() {
        let source = r"use 0x0;
        use 0x1;
        use 0x11;
        use 0x1111;
        use 0x11111111;
        use 0x1111111111111111;
        use 0x00000000000000001111111111111111;";
        let replaced = replace_16_bytes_libra(source, &mut FileSourceMap::default());
        assert_eq!(
            replaced,
            r"use 0x0000000000000000000000000000000000000000;
        use 0x0000000000000000000000000000000000000001;
        use 0x0000000000000000000000000000000000000011;
        use 0x0000000000000000000000000000000000001111;
        use 0x0000000000000000000000000000000011111111;
        use 0x0000000000000000000000001111111111111111;
        use 0x0000000000000000000000001111111111111111;"
        )
    }
}
