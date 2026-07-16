//! Billing demo data Tauri commands (P0-c).
//!
//! Extracted from `commands.rs` as part of S2 P1.1 (commands.rs split).
//! The billing surface is intentionally a demo right now — the UI shows a
//! "Demo mode" banner. These commands return deterministic sample data so the
//! settings page can render end-to-end while the real billing backend is
//! wired in a later phase. Shapes mirror `ui/src/types/index.ts`.

// --- Billing (P0-c) ---------------------------------------------------------
//
// The billing surface is intentionally a demo right now — the UI shows a
// "Demo mode" banner. These commands return deterministic sample data so the
// settings page can render end-to-end while the real billing backend is
// wired in a later phase. Shapes mirror `ui/src/types/index.ts`.

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BillingPlanDto {
    pub name: String,
    pub price: u32,
    pub token_limit: u64,
    pub features: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CostRecordDto {
    pub date: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BillingHistoryDto {
    pub id: String,
    pub date: String,
    pub description: String,
    pub amount: f64,
    pub status: String,
}

#[tauri::command]
pub async fn get_billing_plan() -> Result<BillingPlanDto, String> {
    Ok(BillingPlanDto {
        name: "Pro".into(),
        price: 24,
        token_limit: 2_000_000,
        features: vec![
            "Unlimited sessions".into(),
            "5 concurrent agents".into(),
            "Claude Sonnet + Opus access".into(),
            "MCP marketplace".into(),
            "Priority support".into(),
        ],
    })
}

#[tauri::command]
pub async fn get_cost_history(days: u32) -> Result<Vec<CostRecordDto>, String> {
    let count = days.clamp(1, 90) as usize;
    let mut out = Vec::with_capacity(count);
    for i in (0..count).rev() {
        let base = 8.0 + ((i as f64) / 2.0).sin() * 3.0;
        let cost = ((base * 100.0).round()) / 100.0;
        let date = iso_days_ago(i as i64);
        out.push(CostRecordDto {
            date,
            input_tokens: (cost * 25_000.0) as u64,
            output_tokens: (cost * 8_000.0) as u64,
            cost_usd: cost.max(2.0),
        });
    }
    Ok(out)
}

#[tauri::command]
pub async fn get_billing_history() -> Result<Vec<BillingHistoryDto>, String> {
    let months = ["June", "May", "April", "March", "February", "January"];
    let year = 2026;
    let mut out = Vec::with_capacity(months.len());
    for (i, m) in months.iter().enumerate() {
        let amount = if *m == "February" { 38.0 } else { 24.0 };
        out.push(BillingHistoryDto {
            id: format!("inv-{year}-{:02}", i + 1),
            date: iso_days_ago((i as i64) * 30),
            description: format!("Pro plan — {m} {year}"),
            amount,
            status: "paid".into(),
        });
    }
    Ok(out)
}

pub(crate) fn iso_days_ago(days: i64) -> String {
    use chrono::{DateTime, Days, Utc};
    let now: DateTime<Utc> = Utc::now();
    let target = now
        .checked_sub_days(Days::new(days.max(0) as u64))
        .unwrap_or(now);
    target.format("%Y-%m-%d").to_string()
}
