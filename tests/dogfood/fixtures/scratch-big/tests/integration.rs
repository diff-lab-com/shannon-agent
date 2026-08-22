//! End-to-end flow: stock -> order -> pricing -> shipping -> invoice.
//!
//! These tests only use the re-exported public API from `lib.rs`; that is
//! the surface the L-tier migration task must keep green.

use scratch_big_lib::{
    Currency, Customer, CustomerTier, Inventory, Invoice, Money, Order, OrderItem, OrderStatus,
    Zone, price_order, shipping_cost,
};

fn stocked() -> Inventory {
    let mut inv = Inventory::new();
    inv.set_stock("SKU-A", 10);
    inv.set_stock("SKU-B", 5);
    inv
}

fn gold_customer_order() -> (Customer, Order) {
    let customer =
        Customer::new(12, "Grace Hopper", "grace@example.com", "US").with_tier(CustomerTier::Gold);
    let mut order = Order::new(100, 12);
    order.add_item(OrderItem::new("SKU-A", Money::new(1250, Currency::Usd), 2));
    order.add_item(OrderItem::new("SKU-B", Money::new(4000, Currency::Usd), 1));
    order.set_status(OrderStatus::Submitted);
    (customer, order)
}

#[test]
fn full_order_flow_produces_expected_invoice() {
    let mut inv = stocked();
    assert_eq!(inv.reserve("SKU-A", 2), Ok(8));
    assert_eq!(inv.reserve("SKU-B", 1), Ok(4));
    assert_eq!(inv.low_stock(4), vec!["SKU-B"]);

    let (customer, order) = gold_customer_order();
    let pricing = price_order(&order, customer.tier());
    let zone = Zone::from_country(customer.country());
    let shipping = shipping_cost(zone, 4, Currency::Usd);

    // Subtotal 65.00; gold discount 6.50; tax 4.68 on 58.50; total 63.18.
    assert_eq!(pricing.subtotal.cents(), 6500);
    assert_eq!(pricing.discount.cents(), 650);
    assert_eq!(pricing.tax.cents(), 468);
    assert_eq!(pricing.total.cents(), 6318);

    // Domestic 4 kg: (500 + 120*4) * 1.0 = 9.80.
    assert_eq!(shipping.cents(), 980);

    let invoice = Invoice::new(&customer, &order, &pricing, zone, shipping);
    let text = invoice.render();
    assert!(text.contains("#12 Grace Hopper (Gold)"));
    assert!(text.contains("Order #100 (Submitted), ship to: domestic"));
    assert!(text.contains("2 x SKU-A @ 12.50 USD = 25.00 USD"));
    assert!(text.contains("1 x SKU-B @ 40.00 USD = 40.00 USD"));
    assert!(text.contains("Subtotal: 65.00 USD"));
    assert!(text.contains("Discount: 6.50 USD"));
    assert!(text.contains("Tax: 4.68 USD"));
    assert!(text.contains("Shipping: 9.80 USD"));
    // 63.18 + 9.80.
    assert!(text.contains("TOTAL: 72.98 USD"));
    assert_eq!(invoice.grand_total().cents(), 7298);
}

#[test]
fn tier_changes_shift_totals() {
    let (gold, order) = gold_customer_order();
    let standard = Customer::new(gold.id(), gold.name(), gold.email(), gold.country());

    let p_gold = price_order(&order, CustomerTier::Gold);
    let p_std = price_order(&order, standard.tier());

    // Standard: no discount, tax on full 65.00 = 5.20, total 70.20.
    assert_eq!(p_std.discount.cents(), 0);
    assert_eq!(p_std.tax.cents(), 520);
    assert_eq!(p_std.total.cents(), 7020);
    assert_eq!(p_gold.total.cents(), 6318);
    assert_eq!(p_std.total.sub(p_gold.total).cents(), 702);
}

#[test]
fn zone_matrix_drives_cost_and_eta() {
    use scratch_big_lib::eta_days;

    assert_eq!(Zone::from_country("US"), Zone::Domestic);
    assert_eq!(Zone::from_country("DE"), Zone::International);

    // (500 + 120*2) * 1.0 = 740; (500 + 120*3) * 2.5 = 2150.
    assert_eq!(shipping_cost(Zone::Domestic, 2, Currency::Eur).cents(), 740);
    assert_eq!(
        shipping_cost(Zone::International, 3, Currency::Eur).cents(),
        2150
    );
    assert_eq!(eta_days(Zone::Domestic), 2);
    assert_eq!(eta_days(Zone::NorthAmerica), 5);
    assert_eq!(eta_days(Zone::International), 9);
}

#[test]
fn inventory_reserve_against_order_keeps_stock_consistent() {
    let (customer, order) = gold_customer_order();
    let mut inv = stocked();
    for item in order.items() {
        let remaining = inv
            .reserve(item.sku(), item.qty())
            .expect("stock should cover the order");
        assert_eq!(remaining, inv.available(item.sku()));
    }
    let priced = price_order(&order, customer.tier());
    assert_eq!(priced.total.cents(), 6318);
    assert_eq!(inv.total_skus(), 2);
}
