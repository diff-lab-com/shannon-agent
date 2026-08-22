//! Customers and loyalty tiers.

/// Loyalty tier; drives the discount rate during pricing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CustomerTier {
    Standard,
    Silver,
    Gold,
}

impl CustomerTier {
    /// Discount rate applied to the order subtotal (0.0 = none).
    pub fn discount_rate(self) -> f64 {
        match self {
            Self::Standard => 0.00,
            Self::Silver => 0.05,
            Self::Gold => 0.10,
        }
    }

    /// Derive the tier from cumulative loyalty points.
    pub fn from_points(points: u32) -> Self {
        if points >= 100 {
            Self::Gold
        } else if points >= 50 {
            Self::Silver
        } else {
            Self::Standard
        }
    }
}

/// A registered customer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Customer {
    id: u32,
    name: String,
    email: String,
    country: String,
    tier: CustomerTier,
}

impl Customer {
    /// Register a new (Standard-tier) customer.
    pub fn new(id: u32, name: impl Into<String>, email: impl Into<String>,
               country: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            email: email.into(),
            country: country.into(),
            tier: CustomerTier::Standard,
        }
    }

    /// Builder: override the tier.
    pub fn with_tier(mut self, tier: CustomerTier) -> Self {
        self.tier = tier;
        self
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn email(&self) -> &str {
        &self.email
    }

    pub fn country(&self) -> &str {
        &self.country
    }

    pub fn tier(&self) -> CustomerTier {
        self.tier
    }

    /// One-line label used by reports: `#7 Ada Lovelace (Gold)`.
    pub fn display(&self) -> String {
        format!("#{} {} ({:?})", self.id, self.name, self.tier)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Customer {
        Customer::new(7, "Ada Lovelace", "ada@example.com", "US")
    }

    #[test]
    fn new_customers_start_standard() {
        assert_eq!(sample().tier(), CustomerTier::Standard);
        assert_eq!(sample().country(), "US");
    }

    #[test]
    fn with_tier_overrides() {
        let c = sample().with_tier(CustomerTier::Gold);
        assert_eq!(c.tier(), CustomerTier::Gold);
        assert_eq!(c.id(), 7);
    }

    #[test]
    fn tier_thresholds() {
        assert_eq!(CustomerTier::from_points(0), CustomerTier::Standard);
        assert_eq!(CustomerTier::from_points(49), CustomerTier::Standard);
        assert_eq!(CustomerTier::from_points(50), CustomerTier::Silver);
        assert_eq!(CustomerTier::from_points(99), CustomerTier::Silver);
        assert_eq!(CustomerTier::from_points(100), CustomerTier::Gold);
    }

    #[test]
    fn discount_rates() {
        assert_eq!(CustomerTier::Standard.discount_rate(), 0.0);
        assert_eq!(CustomerTier::Silver.discount_rate(), 0.05);
        assert_eq!(CustomerTier::Gold.discount_rate(), 0.10);
    }

    #[test]
    fn display_label() {
        assert_eq!(sample().display(), "#7 Ada Lovelace (Standard)");
    }
}
