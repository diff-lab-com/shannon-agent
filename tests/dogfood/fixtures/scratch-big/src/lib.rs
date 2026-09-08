//! scratch-big: a multi-module order-management crate used as the L-tier
//! dogfood fixture (plan §5.2). The flat module layout below is the
//! migration target's *starting point* — the public API re-exported here is
//! the contract that must survive any restructure.

pub mod currency;
pub mod customer;
pub mod inventory;
pub mod order;
pub mod pricing;
pub mod report;
pub mod shipping;

pub use currency::{Currency, Money};
pub use customer::{Customer, CustomerTier};
pub use inventory::{Inventory, StockError};
pub use order::{Order, OrderItem, OrderStatus};
pub use pricing::{PriceSummary, TAX_RATE, price_order};
pub use report::Invoice;
pub use shipping::{BASE_RATE_CENTS, PER_KG_CENTS, Zone, eta_days, shipping_cost};
