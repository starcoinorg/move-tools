use anyhow::Result;

use crate::base::Dialect;
use crate::lang::gas::libra_cost_table;

use crate::shared::errors::FileSourceMap;
use crate::shared::ProvidedAccountAddress;

use move_core_types::account_address::AccountAddress;
use move_core_types::gas_schedule::CostTable;

#[derive(Default)]
pub struct LibraDialect;

impl Dialect for LibraDialect {
    fn name(&self) -> &str {
        "libra"
    }

    fn normalize_account_address(&self, addr: &str) -> Result<ProvidedAccountAddress> {
        let address = AccountAddress::from_hex_literal(&addr)?;
        let normalized_address = format!("0x{}", address);
        Ok(ProvidedAccountAddress::new(
            addr.to_string(),
            normalized_address.clone(),
            normalized_address,
        ))
    }

    fn replace_addresses(&self, source_text: &str, _source_map: &mut FileSourceMap) -> String {
        // crate::shared::addresses::replace_16_bytes_libra(source_text, source_map)
        source_text.to_string()
    }
}
