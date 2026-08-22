//! Order pricing: subtotal, loyalty discount, sales tax, total.

use crate::currency::Money;
use crate::customer::CustomerTier;
use crate::order::Order;

/// Flat sales tax applied to the post-discount amount.
pub const TAX_RATE: f64 = 0.08;

/// The priced breakdown of one order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriceSummary {
    pub subtotal: Money,
    pub discount: Money,
    pub tax: Money,
    pub total: Money,
}

/// Price an order for a customer tier.
///
/// `total = (subtotal - discount) + tax`, tax computed on the discounted
/// amount. All rounding happens inside [`Money::scale`].
pub fn price_order(order: &Order, tier: CustomerTier) -> PriceSummary {
    let subtotal = order.subtotal();
    let discount = subtotal.scale(tier.discount_rate());
    let taxable = subtotal.sub(discount);
    let tax = taxable.scale(TAX_RATE);
    let total = taxable.add(tax);
    PriceSummary { subtotal, discount, tax, total }
}

/// Convenience: the discount rate a tier earns.
pub fn discount_rate_for(tier: CustomerTier) -> f64 {
    tier.discount_rate()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::currency::Currency;
    use crate::order::OrderItem;

    fn order_of(cents: i64, qty: u32) -> Order {
        let mut o = Order::new(1, 1);
        o.add_item(OrderItem::new(
            "SKU-1",
            Money::new(cents, Currency::Usd),
            qty,
        ));
        o
    }

    #[test]
    fn standard_pays_no_discount() {
        let p = price_order(&order_of(10_000, 1), CustomerTier::Standard);
        assert_eq!(p.subtotal.cents(), 10_000);
        assert_eq!(p.discount.cents(), 0);
    }

    #[test]
    fn gold_discount_is_ten_percent_rounded() {
        // 10% of 10.05 (1005c) = 100.5c -> rounds to 101 (half away from zero).
        let p = price_order(&order_of(1005, 1), CustomerTier::Gold);
        assert_eq!(p.discount.cents(), 101);
    }

    #[test]
    fn tax_applies_to_discounted_amount() {
        // subtotal 100.00, gold discount 10.00 -> taxable 90.00, tax 7.20.
        let p = price_order(&order_of(10_000, 1), CustomerTier::Gold);
        assert_eq!(p.tax.cents(), 720);
        assert_eq!(p.total.cents(), 90_00 + 720);
    }

    #[test]
    fn empty_order_prices_to_zero() {
        let p = price_order(&Order::new(1, 1), CustomerTier::Silver);
        assert_eq!(p.subtotal.cents(), 0);
        assert_eq!(p.total.cents(), 0);
    }

    #[test]
    fn rate_delegation() {
        assert_eq!(discount_rate_for(CustomerTier::Silver), 0.05);
    }
}
