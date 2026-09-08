//! Stock levels, reservation, and low-stock alerts.

use std::collections::BTreeMap;

/// Failures from stock operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StockError {
    UnknownSku,
    Insufficient { requested: u32, available: u32 },
}

/// SKU -> on-hand quantity. BTreeMap keeps `low_stock` deterministic.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Inventory {
    stock: BTreeMap<String, u32>,
}

impl Inventory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set (or introduce) a SKU's on-hand quantity.
    pub fn set_stock(&mut self, sku: impl Into<String>, qty: u32) {
        self.stock.insert(sku.into(), qty);
    }

    /// On-hand quantity; 0 for unknown SKUs.
    pub fn available(&self, sku: &str) -> u32 {
        self.stock.get(sku).copied().unwrap_or(0)
    }

    /// Deduct `qty` for a known SKU with sufficient stock; returns the
    /// remaining on-hand quantity.
    pub fn reserve(&mut self, sku: &str, qty: u32) -> Result<u32, StockError> {
        let on_hand = self.stock.get_mut(sku).ok_or(StockError::UnknownSku)?;
        if *on_hand < qty {
            return Err(StockError::Insufficient {
                requested: qty,
                available: *on_hand,
            });
        }
        *on_hand -= qty;
        Ok(*on_hand)
    }

    /// Add stock to a known SKU; returns the new level.
    pub fn restock(&mut self, sku: &str, qty: u32) -> Result<u32, StockError> {
        let on_hand = self.stock.get_mut(sku).ok_or(StockError::UnknownSku)?;
        *on_hand += qty;
        Ok(*on_hand)
    }

    /// Number of tracked SKUs.
    pub fn total_skus(&self) -> usize {
        self.stock.len()
    }

    /// SKUs at or below `threshold`, sorted (BTreeMap order).
    pub fn low_stock(&self, threshold: u32) -> Vec<String> {
        self.stock
            .iter()
            .filter(|(_, &qty)| qty <= threshold)
            .map(|(sku, _)| sku.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stocked() -> Inventory {
        let mut inv = Inventory::new();
        inv.set_stock("SKU-1", 10);
        inv.set_stock("SKU-2", 2);
        inv.set_stock("SKU-3", 0);
        inv
    }

    #[test]
    fn unknown_sku_is_zero() {
        let inv = Inventory::new();
        assert_eq!(inv.available("NOPE"), 0);
        assert_eq!(inv.total_skus(), 0);
    }

    #[test]
    fn reserve_deducts_and_returns_remaining() {
        let mut inv = stocked();
        assert_eq!(inv.reserve("SKU-1", 4), Ok(6));
        assert_eq!(inv.available("SKU-1"), 6);
    }

    #[test]
    fn reserve_reports_insufficient_payload() {
        let mut inv = stocked();
        assert_eq!(
            inv.reserve("SKU-2", 3),
            Err(StockError::Insufficient { requested: 3, available: 2 })
        );
    }

    #[test]
    fn reserve_unknown_sku() {
        let mut inv = stocked();
        assert_eq!(inv.reserve("NOPE", 1), Err(StockError::UnknownSku));
    }

    #[test]
    fn restock_adds_and_rejects_unknown() {
        let mut inv = stocked();
        assert_eq!(inv.restock("SKU-3", 5), Ok(5));
        assert_eq!(inv.restock("NOPE", 5), Err(StockError::UnknownSku));
    }

    #[test]
    fn low_stock_is_sorted_and_inclusive() {
        let inv = stocked();
        assert_eq!(inv.low_stock(2), vec!["SKU-2", "SKU-3"]);
        assert_eq!(inv.low_stock(0), vec!["SKU-3"]);
        assert!(inv.low_stock(0).len() == 1);
    }
}
