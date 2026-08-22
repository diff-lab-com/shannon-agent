//! Invoice rendering: turn a priced order into a text document.

use crate::currency::Money;
use crate::customer::Customer;
use crate::order::Order;
use crate::pricing::PriceSummary;
use crate::shipping::Zone;

/// Everything needed to render one invoice.
#[derive(Debug, Clone)]
pub struct Invoice<'a> {
    customer: &'a Customer,
    order: &'a Order,
    pricing: &'a PriceSummary,
    zone: Zone,
    shipping: Money,
}

impl<'a> Invoice<'a> {
    pub fn new(customer: &'a Customer, order: &'a Order,
               pricing: &'a PriceSummary, zone: Zone, shipping: Money) -> Self {
        Self { customer, order, pricing, zone, shipping }
    }

    pub fn customer(&self) -> &Customer {
        self.customer
    }

    pub fn order(&self) -> &Order {
        self.order
    }

    pub fn zone(&self) -> Zone {
        self.zone
    }

    /// Grand total: priced total plus shipping.
    pub fn grand_total(&self) -> Money {
        self.pricing.total.add(self.shipping)
    }

    /// Render the full text invoice. Line order is fixed so output is
    /// byte-stable for the same inputs.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("=== INVOICE ===\n");
        out.push_str(&format!("Bill to: {}\n", self.customer.display()));
        out.push_str(&format!(
            "Email: {}\n",
            self.customer.email()
        ));
        out.push_str(&format!(
            "Order #{} ({:?}), ship to: {}\n",
            self.order.id(),
            self.order.status(),
            self.zone.label()
        ));
        out.push_str("--- items ---\n");
        for item in self.order.items() {
            out.push_str(&render_line(
                item.sku(),
                item.qty(),
                item.unit_price(),
                item.line_total(),
            ));
        }
        out.push_str("--- totals ---\n");
        out.push_str(&format!("Subtotal: {}\n", self.pricing.subtotal.format()));
        out.push_str(&format!("Discount: {}\n", self.pricing.discount.format()));
        out.push_str(&format!("Tax: {}\n", self.pricing.tax.format()));
        out.push_str(&format!("Shipping: {}\n", self.shipping.format()));
        out.push_str(&format!("TOTAL: {}", self.grand_total().format()));
        out
    }
}

/// Render one item line: `2 x SKU-1 @ 2.50 USD = 5.00 USD`.
pub fn render_line(sku: &str, qty: u32, unit: Money, total: Money) -> String {
    format!(
        "{qty} x {sku} @ {} = {}\n",
        unit.format(),
        total.format()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::currency::Currency;
    use crate::customer::CustomerTier;
    use crate::order::{Order, OrderItem, OrderStatus};
    use crate::pricing::price_order;
    use crate::shipping::shipping_cost;

    fn fixture() -> String {
        let customer = Customer::new(7, "Grace Hopper", "grace@example.com", "US")
            .with_tier(CustomerTier::Gold);
        let mut order = Order::new(3, 7);
        order.add_item(OrderItem::new(
            "SKU-1",
            Money::new(500, Currency::Usd),
            2,
        ));
        order.set_status(OrderStatus::Submitted);
        let pricing = price_order(&order, customer.tier());
        let shipping = shipping_cost(Zone::Domestic, 1, Currency::Usd);
        Invoice::new(&customer, &order, &pricing, Zone::Domestic, shipping)
            .render()
    }

    #[test]
    fn render_contains_every_section() {
        let text = fixture();
        assert!(text.starts_with("=== INVOICE ==="));
        assert!(text.contains("Grace Hopper"));
        assert!(text.contains("grace@example.com"));
        assert!(text.contains("Order #3"));
        assert!(text.contains("domestic"));
        assert!(text.contains("2 x SKU-1 @ 5.00 USD = 10.00 USD"));
        assert!(text.contains("Subtotal: 10.00 USD"));
        assert!(text.contains("Discount: 1.00 USD"));
        assert!(text.contains("Tax: 0.72 USD"));
        // 9.72 priced total + 6.20 domestic shipping (1 kg) = 15.92.
        assert!(text.contains("TOTAL: 15.92 USD"));
    }

    #[test]
    fn line_format() {
        let line = render_line(
            "SKU-9",
            3,
            Money::new(125, Currency::Eur),
            Money::new(375, Currency::Eur),
        );
        assert_eq!(line, "3 x SKU-9 @ 1.25 EUR = 3.75 EUR\n");
    }

    #[test]
    fn grand_total_adds_shipping() {
        let customer = Customer::new(1, "A", "a@example.com", "US");
        let mut order = Order::new(1, 1);
        order.add_item(OrderItem::new(
            "SKU-1",
            Money::new(10_000, Currency::Usd),
            1,
        ));
        let pricing = price_order(&order, customer.tier());
        let shipping = Money::new(740, Currency::Usd);
        let inv = Invoice::new(&customer, &order, &pricing, Zone::Domestic, shipping);
        // 100.00 - 0 discount + 8.00 tax = 108.00; + 7.40 shipping = 115.40.
        assert_eq!(inv.grand_total().cents(), 11_540);
    }
}
