//! Shipping zones, rates, and delivery estimates.

use crate::currency::{Currency, Money};

/// Flat handling fee before distance/weight, in cents.
pub const BASE_RATE_CENTS: i64 = 500;
/// Per-kilogram charge, in cents.
pub const PER_KG_CENTS: i64 = 120;

/// Destination zone derived from the country code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    Domestic,
    NorthAmerica,
    International,
}

impl Zone {
    /// Map an ISO country code to a zone.
    pub fn from_country(code: &str) -> Self {
        match code {
            "US" => Self::Domestic,
            "CA" | "MX" => Self::NorthAmerica,
            _ => Self::International,
        }
    }

    /// Cost multiplier applied to (base + weight) cents.
    pub fn multiplier(self) -> f64 {
        match self {
            Self::Domestic => 1.0,
            Self::NorthAmerica => 1.5,
            Self::International => 2.5,
        }
    }

    /// Report label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Domestic => "domestic",
            Self::NorthAmerica => "north-america",
            Self::International => "international",
        }
    }
}

/// Shipping cost = (base + per-kg * weight) * zone multiplier, rounded.
pub fn shipping_cost(zone: Zone, weight_kg: u32, currency: Currency) -> Money {
    let raw = BASE_RATE_CENTS + PER_KG_CENTS * weight_kg as i64;
    Money::new(raw, currency).scale(zone.multiplier())
}

/// Estimated delivery window in days.
pub fn eta_days(zone: Zone) -> u32 {
    match zone {
        Zone::Domestic => 2,
        Zone::NorthAmerica => 5,
        Zone::International => 9,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zone_mapping() {
        assert_eq!(Zone::from_country("US"), Zone::Domestic);
        assert_eq!(Zone::from_country("CA"), Zone::NorthAmerica);
        assert_eq!(Zone::from_country("MX"), Zone::NorthAmerica);
        assert_eq!(Zone::from_country("JP"), Zone::International);
        assert_eq!(Zone::from_country(""), Zone::International);
    }

    #[test]
    fn domestic_cost_is_flat_plus_weight() {
        // (500 + 120*2) * 1.0 = 740
        assert_eq!(
            shipping_cost(Zone::Domestic, 2, Currency::Usd).cents(),
            740
        );
    }

    #[test]
    fn international_cost_scales_and_rounds() {
        // (500 + 120*3) * 2.5 = 860 * 2.5 = 2150
        assert_eq!(
            shipping_cost(Zone::International, 3, Currency::Usd).cents(),
            2150
        );
    }

    #[test]
    fn na_multiplier_between() {
        // (500 + 0) * 1.5 = 750
        assert_eq!(
            shipping_cost(Zone::NorthAmerica, 0, Currency::Eur).cents(),
            750
        );
    }

    #[test]
    fn eta_per_zone() {
        assert_eq!(eta_days(Zone::Domestic), 2);
        assert_eq!(eta_days(Zone::NorthAmerica), 5);
        assert_eq!(eta_days(Zone::International), 9);
    }

    #[test]
    fn labels() {
        assert_eq!(Zone::Domestic.label(), "domestic");
        assert_eq!(Zone::International.label(), "international");
    }
}
