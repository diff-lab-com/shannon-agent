//! Money as integer minor units (cents) with a currency code.
//!
//! Integer cents avoid float drift; all percent math goes through
//! [`Money::scale`], which rounds half-away-from-zero at the boundary.

/// Supported currencies for this crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Currency {
    Usd,
    Eur,
}

impl Currency {
    /// ISO 4217 alpha code.
    pub fn code(self) -> &'static str {
        match self {
            Self::Usd => "USD",
            Self::Eur => "EUR",
        }
    }
}

/// An amount of money in minor units (cents), tagged with a currency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Money {
    cents: i64,
    code: Currency,
}

impl Money {
    /// Build from minor units.
    pub fn new(cents: i64, code: Currency) -> Self {
        Self { cents, code }
    }

    /// The zero amount for a currency.
    pub fn zero(code: Currency) -> Self {
        Self { cents: 0, code }
    }

    /// Minor units.
    pub fn cents(self) -> i64 {
        self.cents
    }

    /// Currency tag.
    pub fn code(self) -> Currency {
        self.code
    }

    /// Add two same-currency amounts; `None` on currency mismatch.
    pub fn checked_add(self, other: Money) -> Option<Money> {
        if self.code != other.code {
            return None;
        }
        Some(Self::new(self.cents + other.cents, self.code))
    }

    /// Add two amounts; panics on currency mismatch.
    pub fn add(self, other: Money) -> Money {
        self.checked_add(other).expect("currency mismatch in Money::add")
    }

    /// Subtract `other` from `self`; `None` on currency mismatch.
    pub fn checked_sub(self, other: Money) -> Option<Money> {
        if self.code != other.code {
            return None;
        }
        Some(Self::new(self.cents - other.cents, self.code))
    }

    /// Subtract; panics on currency mismatch.
    pub fn sub(self, other: Money) -> Money {
        self.checked_sub(other).expect("currency mismatch in Money::sub")
    }

    /// Scale by a factor (discount rates, tax rates, zone multipliers),
    /// rounding half-away-from-zero.
    pub fn scale(self, factor: f64) -> Money {
        Self::new((self.cents as f64 * factor).round() as i64, self.code)
    }

    /// Human-readable form, e.g. `12.34 USD` / `-3.00 EUR`.
    pub fn format(self) -> String {
        let sign = if self.cents < 0 { "-" } else { "" };
        let abs = self.cents.unsigned_abs();
        format!("{sign}{}.{:02} {}", abs / 100, abs % 100, self.code.code())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_and_sub_same_currency() {
        let a = Money::new(199, Currency::Usd);
        let b = Money::new(1, Currency::Usd);
        assert_eq!(a.add(b).cents(), 200);
        assert_eq!(a.sub(b).cents(), 198);
    }

    #[test]
    fn checked_ops_reject_mismatched_currency() {
        let a = Money::new(100, Currency::Usd);
        let b = Money::new(100, Currency::Eur);
        assert!(a.checked_add(b).is_none());
        assert!(a.checked_sub(b).is_none());
    }

    #[test]
    fn scale_rounds_half_away_from_zero() {
        let m = Money::new(105, Currency::Usd);
        assert_eq!(m.scale(0.10).cents(), 11); // 10.5 -> 11
        assert_eq!(m.scale(-0.10).cents(), -11);
    }

    #[test]
    fn format_positive_and_negative() {
        assert_eq!(Money::new(1234, Currency::Usd).format(), "12.34 USD");
        assert_eq!(Money::new(-300, Currency::Eur).format(), "-3.00 EUR");
    }

    #[test]
    fn zero_is_zero() {
        assert_eq!(Money::zero(Currency::Usd).cents(), 0);
        assert_eq!(Money::zero(Currency::Usd).code(), Currency::Usd);
    }
}
