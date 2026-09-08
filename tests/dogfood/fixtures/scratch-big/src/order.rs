//! Orders: a customer's line items plus lifecycle status.

use crate::currency::Money;

/// One line item: SKU, unit price, quantity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderItem {
    sku: String,
    unit_price: Money,
    qty: u32,
}

impl OrderItem {
    pub fn new(sku: impl Into<String>, unit_price: Money, qty: u32) -> Self {
        Self { sku: sku.into(), unit_price, qty }
    }

    pub fn sku(&self) -> &str {
        &self.sku
    }

    pub fn unit_price(&self) -> Money {
        self.unit_price
    }

    pub fn qty(&self) -> u32 {
        self.qty
    }

    /// Unit price times quantity.
    pub fn line_total(&self) -> Money {
        self.unit_price.scale(self.qty as f64)
    }
}

/// Order lifecycle. Draft -> Submitted -> Shipped is the only legal path;
/// the setters do not enforce it (reports render any state).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderStatus {
    Draft,
    Submitted,
    Shipped,
}

/// A customer's order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Order {
    id: u32,
    customer_id: u32,
    items: Vec<OrderItem>,
    status: OrderStatus,
}

impl Order {
    /// An empty Draft order.
    pub fn new(id: u32, customer_id: u32) -> Self {
        Self { id, customer_id, items: Vec::new(), status: OrderStatus::Draft }
    }

    pub fn add_item(&mut self, item: OrderItem) {
        self.items.push(item);
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn customer_id(&self) -> u32 {
        self.customer_id
    }

    pub fn items(&self) -> &[OrderItem] {
        &self.items
    }

    pub fn status(&self) -> OrderStatus {
        self.status
    }

    pub fn set_status(&mut self, status: OrderStatus) {
        self.status = status;
    }

    /// Number of distinct line items.
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// Total units across line items.
    pub fn total_qty(&self) -> u32 {
        self.items.iter().map(|i| i.qty).sum()
    }

    /// Sum of line totals. Empty order => zero of the first item's
    /// currency, or USD when there are no items yet.
    pub fn subtotal(&self) -> Money {
        let code = self
            .items
            .first()
            .map(|i| i.unit_price().code())
            .unwrap_or(crate::currency::Currency::Usd);
        self.items
            .iter()
            .fold(Money::zero(code), |acc, i| acc.add(i.line_total()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::currency::Currency;

    fn item(sku: &str, cents: i64, qty: u32) -> OrderItem {
        OrderItem::new(sku, Money::new(cents, Currency::Usd), qty)
    }

    #[test]
    fn empty_order_is_zero_usd() {
        let o = Order::new(1, 42);
        assert_eq!(o.subtotal().cents(), 0);
        assert_eq!(o.subtotal().code(), Currency::Usd);
        assert_eq!(o.status(), OrderStatus::Draft);
        assert_eq!(o.total_qty(), 0);
    }

    #[test]
    fn line_total_multiplies() {
        assert_eq!(item("SKU-1", 250, 4).line_total().cents(), 1000);
    }

    #[test]
    fn subtotal_sums_lines() {
        let mut o = Order::new(2, 42);
        o.add_item(item("SKU-1", 250, 4)); // 1000
        o.add_item(item("SKU-2", 199, 1)); // 199
        assert_eq!(o.subtotal().cents(), 1199);
        assert_eq!(o.item_count(), 2);
        assert_eq!(o.total_qty(), 5);
    }

    #[test]
    fn status_transitions() {
        let mut o = Order::new(3, 42);
        assert_eq!(o.status(), OrderStatus::Draft);
        o.set_status(OrderStatus::Submitted);
        assert_eq!(o.status(), OrderStatus::Submitted);
        o.set_status(OrderStatus::Shipped);
        assert_eq!(o.status(), OrderStatus::Shipped);
    }

    #[test]
    fn accessors() {
        let o = Order::new(9, 77);
        assert_eq!(o.id(), 9);
        assert_eq!(o.customer_id(), 77);
        assert!(o.items().is_empty());
    }
}
