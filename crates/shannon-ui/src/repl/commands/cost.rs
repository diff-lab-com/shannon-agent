use crate::{widgets::ChatRole, Result};

use super::super::Repl;

pub(crate) fn handle_cost(repl: &mut Repl, args: &str) -> Result<()> {
    let subcmd = args.trim();

    // Handle budget subcommand
    if let Some(budget_str) = subcmd.strip_prefix("budget ") {
        let limit: f64 = match budget_str.trim().parse() {
            Ok(v) if v > 0.0 => v,
            _ => {
                repl.chat.add_message(ChatRole::System, "Usage: /cost budget <amount_usd>".to_string());
                return Ok(());
            }
        };
        if let Some(ref engine) = repl.query_engine {
            if let Ok(mut tracker) = engine.cost_tracker().write() {
                tracker.set_budget(limit);
            }
        }
        repl.chat.add_message(ChatRole::System, format!("Budget limit set to ${limit:.2}"));
        return Ok(());
    }

    let Some(ref engine) = repl.query_engine else {
        repl.chat.add_message(ChatRole::System, "No query engine available.".to_string());
        return Ok(());
    };

    let stats = engine.conversation_stats();
    let model = repl.state.model.as_deref().unwrap_or("unknown");

    // Use detailed report from CostTracker
    let detailed = if let Ok(tracker) = engine.cost_tracker().read() {
        tracker.detailed_report()
    } else {
        format!("Total cost: ${:.4}\n", repl.state.total_cost_usd)
    };

    let mut report = format!(
        "Cost Summary:\n  Model: {}\n  Messages: {} turns\n  Tokens used: {} ({:.1}k)\n  Working dir: {}\n",
        model,
        stats.turn_count,
        repl.state.tokens_used,
        repl.state.tokens_used as f64 / 1000.0,
        repl.state.working_directory,
    );

    report.push_str(&detailed);

    if let Some(started) = &repl.session_started_at {
        let elapsed = chrono::Utc::now() - *started;
        let mins = elapsed.num_minutes();
        let secs = elapsed.num_seconds() % 60;
        report.push_str(&format!("  Session duration: {mins}m {secs}s"));

        if mins > 0 {
            let cost_per_min = repl.state.total_cost_usd / mins as f64;
            report.push_str(&format!("\n  Cost rate: ${cost_per_min:.4}/min"));
        }
    }

    if repl.diff_data.total_files_modified() > 0 || repl.diff_data.total_files_created() > 0 {
        report.push_str(&format!(
            "\n  Files changed: +{}/-{} ({} modified, {} created, {} deleted)",
            repl.diff_data.total_additions(),
            repl.diff_data.total_deletions(),
            repl.diff_data.total_files_modified(),
            repl.diff_data.total_files_created(),
            repl.diff_data.total_files_deleted(),
        ));
    }

    // Budget warning
    if let Ok(tracker) = engine.cost_tracker().read() {
        if let Some(ratio) = tracker.budget_usage_ratio() {
            if ratio >= 1.0 {
                report.push_str("\n  ⚠ BUDGET EXCEEDED");
            } else if ratio >= 0.8 {
                report.push_str(&format!("\n  ⚠ Budget usage: {:.0}%", ratio * 100.0));
            }
        }
    }

    repl.chat.add_message(ChatRole::System, report);
    Ok(())
}

pub(crate) fn handle_suggest(repl: &mut Repl, _args: &str) -> Result<()> {
    let engine = shannon_core::ContextSuggestionEngine::new();

    // Build context from session state
    let mut recently_edited: Vec<String> = Vec::new();
    let mut recently_created: Vec<String> = Vec::new();
    // Collect from last 3 turns
    for turn in repl.diff_data.turns.iter().rev().take(3) {
        for fc in &turn.files_modified {
            if !recently_edited.contains(&fc.path) {
                recently_edited.push(fc.path.clone());
            }
        }
        for f in &turn.files_created {
            if !recently_created.contains(f) {
                recently_created.push(f.clone());
            }
        }
    }

    let context = shannon_core::EnhancedSuggestionContext {
        recently_edited_files: recently_edited,
        recently_created_files: recently_created,
        recently_used_tools: Vec::new(),
        recently_run_commands: Vec::new(),
        working_directory: Some(repl.state.working_directory.clone()),
        open_files: Vec::new(),
    };

    let suggestions = engine.suggest_for_conversation_start(&context);

    if suggestions.is_empty() {
        repl.chat.add_message(ChatRole::System,
            "No suggestions available for the current context.".to_string());
        return Ok(());
    }

    let mut msg = "Suggestions:\n".to_string();
    for (i, s) in suggestions.iter().enumerate() {
        msg.push_str(&format!(
            "  {}. {} (priority: {}, confidence: {:.0}%)\n",
            i + 1, s.reason, s.priority, s.confidence * 100.0
        ));
        if let Some(tool) = &s.suggested_tool {
            msg.push_str(&format!("     Tool: {tool}\n"));
        }
        if !s.suggested_files.is_empty() {
            msg.push_str(&format!("     Files: {}\n", s.suggested_files.join(", ")));
        }
    }
    repl.chat.add_message(ChatRole::System, msg);
    Ok(())
}

pub(crate) fn handle_billing(repl: &mut Repl, args: &str) -> Result<()> {
    let subcmd = args.trim();

    match subcmd {
        "" | "period" => {
            let summary = repl.state.billing_manager.get_period_summary();
            let mut msg = format!(
                "Billing Period: {} to {}\n  Total cost: ${:.4}\n  Input tokens: {}\n  Output tokens: {}\n  Models used: {}",
                summary.start.format("%Y-%m-%d"),
                summary.end.format("%Y-%m-%d"),
                summary.total_cost,
                summary.total_input_tokens,
                summary.total_output_tokens,
                summary.usage_breakdown.len(),
            );
            for (model, usage) in &summary.usage_breakdown {
                msg.push_str(&format!(
                    "\n    {}: ${:.4} ({} req, {} in, {} out)",
                    model, usage.total_cost, usage.request_count,
                    usage.total_input_tokens, usage.total_output_tokens
                ));
            }
            repl.chat.add_message(ChatRole::System, msg);
        }
        "model" => {
            let breakdown = repl.state.billing_manager.get_model_breakdown();
            if breakdown.is_empty() {
                repl.chat.add_message(ChatRole::System, "No billing data recorded yet.".to_string());
                return Ok(());
            }
            let mut msg = "Usage by Model:\n".to_string();
            for (model, usage) in &breakdown {
                msg.push_str(&format!(
                    "  {}: ${:.4} ({} req, {}+{} tokens)\n",
                    model, usage.total_cost, usage.request_count,
                    usage.total_input_tokens, usage.total_output_tokens
                ));
            }
            repl.chat.add_message(ChatRole::System, msg);
        }
        "daily" => {
            let daily = repl.state.billing_manager.get_daily_totals();
            if daily.is_empty() {
                repl.chat.add_message(ChatRole::System, "No billing data recorded yet.".to_string());
                return Ok(());
            }
            let mut msg = "Daily Usage:\n".to_string();
            for d in &daily {
                msg.push_str(&format!(
                    "  {}: ${:.4} ({} req, {}+{} tokens)\n",
                    d.date, d.total_cost, d.request_count,
                    d.total_input_tokens, d.total_output_tokens
                ));
            }
            repl.chat.add_message(ChatRole::System, msg);
        }
        _ if subcmd.starts_with("budget ") => {
            let rest = subcmd.strip_prefix("budget ").unwrap_or("");
            let amount_str = rest.trim();
            if amount_str == "off" || amount_str == "none" {
                let mut cfg = repl.state.billing_manager.config().clone();
                cfg.monthly_budget = None;
                repl.state.billing_manager.set_config(cfg);
                repl.chat.add_message(ChatRole::System, "Monthly budget limit removed.".to_string());
            } else {
                let limit: f64 = match amount_str.parse() {
                    Ok(v) if v > 0.0 => v,
                    _ => {
                        repl.chat.add_message(ChatRole::System,
                            "Usage: /billing budget <amount_usd|off>".to_string());
                        return Ok(());
                    }
                };
                let mut cfg = repl.state.billing_manager.config().clone();
                cfg.monthly_budget = Some(limit);
                repl.state.billing_manager.set_config(cfg);
                repl.chat.add_message(ChatRole::System,
                    format!("Monthly budget limit set to ${limit:.2}"));
            }
        }
        _ => {
            repl.chat.add_message(ChatRole::System,
                "Usage: /billing [period|model|daily|budget <amount|off>]".to_string());
        }
    }

    // Check for pending budget alerts
    let alerts = repl.state.billing_manager.get_alerts().to_vec();
    if !alerts.is_empty() {
        for alert in &alerts {
            repl.chat.add_message(ChatRole::System, format!("⚠️ {}", alert.message));
        }
        repl.state.billing_manager.clear_alerts();
    }

    Ok(())
}

pub(crate) fn handle_plan(repl: &mut Repl, args: &str) -> Result<()> {
    let parts: Vec<&str> = args.split_whitespace().collect();

    match parts.first().copied().unwrap_or("") {
        "" | "status" => {
            let plan = &repl.state.plan;
            if !plan.active {
                repl.chat.add_message(ChatRole::System,
                    "No active plan. Use /plan <description> to create one.".to_string());
                return Ok(());
            }
            let status = if plan.approved { "Approved" } else { "Pending review" };
            let mut msg = format!(
                "Plan: {}\nStatus: {}\n\n{}",
                plan.description, status, plan.content
            );
            if plan.approved {
                msg.push_str("\n\nPlan approved — implementation can proceed.");
            } else {
                msg.push_str("\n\nUse /plan approve to approve, /plan reject to discard.");
            }
            repl.chat.add_message(ChatRole::System, msg);
        }
        "approve" => {
            if !repl.state.plan.active {
                repl.chat.add_message(ChatRole::System, "No active plan to approve.".to_string());
                return Ok(());
            }
            repl.state.plan.approved = true;
            repl.state.status = "Plan approved".to_string();
            // Save plan to disk
            let plan_dir = std::path::Path::new(&repl.state.working_directory)
                .join(".claude").join("plans");
            let _ = std::fs::create_dir_all(&plan_dir);
            let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
            let plan_file = plan_dir.join(format!("plan_{timestamp}.md"));
            let content = format!("# Plan: {}\n\n{}", repl.state.plan.description, repl.state.plan.content);
            let _ = std::fs::write(&plan_file, content);
            repl.chat.add_message(ChatRole::System,
                format!("Plan approved and saved. You can now proceed with implementation.\nSaved to: {}",
                    plan_file.display()));
        }
        "reject" => {
            if !repl.state.plan.active {
                repl.chat.add_message(ChatRole::System, "No active plan to reject.".to_string());
                return Ok(());
            }
            repl.state.plan = super::super::PlanState::default();
            repl.state.status = "Ready".to_string();
            repl.chat.add_message(ChatRole::System, "Plan rejected and cleared.".to_string());
        }
        "done" => {
            if !repl.state.plan.active {
                repl.chat.add_message(ChatRole::System, "No active plan.".to_string());
                return Ok(());
            }
            let desc = repl.state.plan.description.clone();
            repl.state.plan = super::super::PlanState::default();
            repl.state.status = "Ready".to_string();
            repl.chat.add_message(ChatRole::System,
                format!("Plan '{desc}' completed and cleared."));
        }
        "help" => {
            repl.chat.add_message(ChatRole::System,
                "Plan Commands:\n\
                 /plan <description> — Create a new plan from a description\n\
                 /plan status — Show current plan\n\
                 /plan approve — Approve the current plan\n\
                 /plan reject — Reject and discard the current plan\n\
                 /plan done — Mark plan as completed\n\
                 /plan help — Show this help".to_string());
        }
        // Treat anything else as a plan description
        _ => {
            let description = args.trim().to_string();
            if description.is_empty() {
                repl.chat.add_message(ChatRole::System, "Usage: /plan <description>".to_string());
                return Ok(());
            }
            // Generate a structured plan
            let plan_content = generate_plan(&description);
            repl.state.plan = super::super::PlanState {
                active: true,
                content: plan_content.clone(),
                description: description.clone(),
                approved: false,
                scroll_offset: 0,
            };
            repl.state.status = "Plan mode — review plan".to_string();
            let msg = format!(
                "Plan created: {description}\n\n{plan_content}\n\nUse /plan approve to approve, /plan reject to discard, or /plan help for more options."
            );
            repl.chat.add_message(ChatRole::System, msg);
        }
    }

    Ok(())
}

/// Generate a structured plan from a description
pub(crate) fn generate_plan(description: &str) -> String {
    let steps = extract_plan_steps(description);
    let mut plan = String::from("## Implementation Steps\n\n");
    for (i, step) in steps.iter().enumerate() {
        plan.push_str(&format!("{}. {}\n", i + 1, step));
    }
    plan.push_str("\n## Acceptance Criteria\n\n");
    plan.push_str("- All steps completed successfully\n");
    plan.push_str("- Tests pass for new functionality\n");
    plan.push_str("- No regressions in existing tests\n");
    plan
}

/// Extract plan steps from a description using heuristic keyword detection
pub(crate) fn extract_plan_steps(description: &str) -> Vec<String> {
    let mut steps = Vec::new();

    // Detect common patterns and generate appropriate steps
    let lower = description.to_lowercase();

    if lower.contains("refactor") || lower.contains("restructure") {
        steps.push("Analyze current architecture and identify components to refactor".to_string());
        steps.push("Design new structure with clear separation of concerns".to_string());
        steps.push("Implement refactoring incrementally, keeping tests green".to_string());
        steps.push("Update all references and imports".to_string());
        steps.push("Run full test suite to verify no regressions".to_string());
    }

    if lower.contains("test") || lower.contains("coverage") {
        steps.push("Identify untested code paths and edge cases".to_string());
        steps.push("Write unit tests for core logic".to_string());
        steps.push("Write integration tests for component interactions".to_string());
        steps.push("Verify test coverage meets threshold".to_string());
    }

    if (lower.contains("add") || lower.contains("implement") || lower.contains("feature"))
        && steps.is_empty() {
            steps.push("Analyze requirements and design interface".to_string());
            steps.push("Implement core functionality".to_string());
            steps.push("Add error handling and input validation".to_string());
            steps.push("Write tests for new functionality".to_string());
            steps.push("Update documentation".to_string());
        }

    if (lower.contains("fix") || lower.contains("bug"))
        && steps.is_empty() {
            steps.push("Reproduce the issue and understand root cause".to_string());
            steps.push("Implement fix with minimal changes".to_string());
            steps.push("Add regression test".to_string());
            steps.push("Verify fix resolves the issue".to_string());
        }

    if (lower.contains("migrate") || lower.contains("upgrade"))
        && steps.is_empty() {
            steps.push("Review migration/upgrade guide and breaking changes".to_string());
            steps.push("Update dependencies".to_string());
            steps.push("Adapt code to new API surface".to_string());
            steps.push("Run tests and fix any failures".to_string());
            steps.push("Verify functionality end-to-end".to_string());
        }

    // Default fallback
    if steps.is_empty() {
        steps.push(format!("Understand requirements: {description}"));
        steps.push("Design solution approach".to_string());
        steps.push("Implement the solution".to_string());
        steps.push("Test and verify the implementation".to_string());
    }

    steps
}

pub(crate) fn handle_permissions(repl: &mut Repl, args: &str) -> Result<()> {
    use shannon_core::permissions::RiskLevel;

    let parts: Vec<&str> = args.split_whitespace().collect();

    // Subcommand dispatch
    match parts.first().copied().unwrap_or("") {
        "" | "status" => {
            let mut report = String::from("Permission Status:\n");

            if let Some(ref engine) = repl.query_engine {
                if let Ok(perms) = engine.permissions().read() {
                    // Tool policies
                    report.push_str(&format!("  Registered policies: {}\n", perms.tool_policies().len()));
                    let mut policies: Vec<_> = perms.tool_policies().iter().collect();
                    policies.sort_by_key(|(name, _)| name.as_str());
                    for (name, policy) in &policies {
                        let risk = match policy.default_risk_level {
                            RiskLevel::Safe => "Safe",
                            RiskLevel::Low => "Low",
                            RiskLevel::Medium => "Medium",
                            RiskLevel::High => "High",
                            RiskLevel::Critical => "Critical",
                        };
                        let deny_count = policy.deny_patterns.len();
                        let confirm_count = policy.confirmation_patterns.len();
                        report.push_str(&format!(
                            "    {name}: {risk} risk, {deny_count} deny patterns, {confirm_count} confirm patterns\n"
                        ));
                    }

                    // Always-allowed tools
                    let allowed = perms.memory().always_allowed_tools();
                    if !allowed.is_empty() {
                        let mut tools: Vec<&str> = allowed.iter().map(|s| s.as_str()).collect();
                        tools.sort();
                        report.push_str(&format!("  Always allowed: {}\n", tools.join(", ")));
                    }

                    // Always-denied tools
                    let denied = perms.memory().always_denied_tools();
                    if !denied.is_empty() {
                        let mut tools: Vec<&str> = denied.iter().map(|s| s.as_str()).collect();
                        tools.sort();
                        report.push_str(&format!("  Always denied: {}\n", tools.join(", ")));
                    }

                    if allowed.is_empty() && denied.is_empty() {
                        report.push_str("  No tool-level overrides (using defaults)\n");
                    }
                }
            } else {
                report.push_str("  No query engine available.\n");
            }

            repl.chat.add_message(ChatRole::System, report);
        }
        "allow" => {
            if parts.len() < 2 {
                repl.chat.add_message(ChatRole::System, "Usage: /permissions allow <tool_name>".to_string());
                return Ok(());
            }
            let tool = parts[1];
            if let Some(ref engine) = repl.query_engine {
                if let Ok(mut perms) = engine.permissions().write() {
                    perms.allow_tool(tool);
                }
            }
            repl.chat.add_message(ChatRole::System, format!("Tool '{tool}' is now always allowed."));
        }
        "deny" => {
            if parts.len() < 2 {
                repl.chat.add_message(ChatRole::System, "Usage: /permissions deny <tool_name>".to_string());
                return Ok(());
            }
            let tool = parts[1];
            if let Some(ref engine) = repl.query_engine {
                if let Ok(mut perms) = engine.permissions().write() {
                    perms.deny_tool(tool);
                }
            }
            repl.chat.add_message(ChatRole::System, format!("Tool '{tool}' is now always denied."));
        }
        "reset" => {
            if let Some(ref engine) = repl.query_engine {
                if let Ok(mut perms) = engine.permissions().write() {
                    perms.reset_memory();
                }
            }
            repl.chat.add_message(ChatRole::System, "Permission memory cleared. All tool overrides removed.".to_string());
        }
        "mode" => {
            let mode_name = parts.get(1).copied().unwrap_or("");
            match mode_name {
                "strict" | "suggest" => {
                    if let Some(ref engine) = repl.query_engine {
                        if let Ok(mut perms) = engine.permissions().write() {
                            perms.set_approval_mode(shannon_core::permissions::ApprovalMode::Suggest);
                        }
                    }
                    repl.state.approval_mode_label = "SUGGEST".to_string();
                    repl.chat.add_message(ChatRole::System,
                        "Permission mode: **suggest** (strict)\n\
                         All potentially dangerous tools require explicit approval.".to_string());
                }
                "auto" | "auto-accept" | "yolo" | "full-auto" => {
                    if let Some(ref engine) = repl.query_engine {
                        if let Ok(mut perms) = engine.permissions().write() {
                            perms.set_approval_mode(shannon_core::permissions::ApprovalMode::FullAuto);
                        }
                    }
                    repl.state.approval_mode_label = "FULL".to_string();
                    repl.chat.add_message(ChatRole::System,
                        "Permission mode: **full-auto**\n\
                         All tools are automatically approved. Use with caution.".to_string());
                }
                "plan" | "readonly" => {
                    if let Some(ref engine) = repl.query_engine {
                        if let Ok(mut perms) = engine.permissions().write() {
                            perms.set_approval_mode(shannon_core::permissions::ApprovalMode::Readonly);
                        }
                    }
                    repl.state.approval_mode_label = "RO".to_string();
                    repl.chat.add_message(ChatRole::System,
                        "Permission mode: **readonly**\n\
                         Tools will only read, not modify files.".to_string());
                }
                _ => {
                    repl.chat.add_message(ChatRole::System,
                        "Permission Modes:\n\
                         /permissions mode suggest   — Require approval for dangerous tools\n\
                         /permissions mode auto      — Auto-accept all tool executions\n\
                         /permissions mode readonly  — Read-only, no file modifications".to_string());
                }
            }
        }
        _ => {
            repl.chat.add_message(ChatRole::System,
                "Permission Commands:\n\
                 /permissions status — Show current permission policies and overrides\n\
                 /permissions allow <tool> — Always allow a tool without prompting\n\
                 /permissions deny <tool> — Always deny a tool\n\
                 /permissions reset — Clear all permission overrides\n\
                 /permissions mode [suggest|auto|readonly] — Change approval mode\n\
                 /permissions help — Show this help".to_string());
        }
    }

    Ok(())
}
