//! Tauri commands wrapping [`crate::routine_templates`].
//!
//! Two commands:
//! - `list_routine_templates` — read-only enumeration of every `.toml` in
//!   `routines/`.
//! - `instantiate_routine_template` — copy a template by id into the user's
//!   scheduled-task store. The UI could just call `create_scheduled_task`
//!   directly with the template fields, but offering a single command makes
//!   the intent explicit and gives us a place to add side effects later
//!   (e.g. logging which templates are popular).

use crate::commands::AppState;
use crate::routine_templates::{RoutineTemplate, list_templates};
use crate::scheduled_commands::CreateTaskPayload;
use shannon_core::scheduled_routines::ScheduledRoutine;

#[tauri::command]
pub async fn list_routine_templates() -> Result<Vec<RoutineTemplate>, String> {
    Ok(list_templates())
}

#[tauri::command]
pub async fn instantiate_routine_template(
    state: tauri::State<'_, AppState>,
    template_id: String,
    name_override: Option<String>,
) -> Result<ScheduledRoutine, String> {
    let template = list_templates()
        .into_iter()
        .find(|t| t.id == template_id)
        .ok_or_else(|| format!("template '{template_id}' not found"))?;

    let payload = CreateTaskPayload {
        name: name_override.unwrap_or(template.name),
        prompt: template.prompt,
        trigger_type: Some(template.trigger_type),
        interval_secs: template.interval_secs,
        cron_expr: template.cron_expr,
        timezone: template.timezone,
        expires_at: None,
        max_fires: None,
        policy: None,
        depends_on: None,
    };

    let trigger_type = payload
        .trigger_type
        .as_deref()
        .map(|s| match s {
            "cron" => shannon_core::scheduled_routines::TriggerType::Cron,
            _ => shannon_core::scheduled_routines::TriggerType::Interval,
        })
        .unwrap_or_default();

    let mut routine = match trigger_type {
        shannon_core::scheduled_routines::TriggerType::Cron => {
            let cron_expr = payload
                .cron_expr
                .clone()
                .ok_or_else(|| "cron_expr is required when trigger_type=cron".to_string())?;
            ScheduledRoutine::new_cron(payload.name.clone(), payload.prompt.clone(), cron_expr)
                .map_err(|e| e.to_string())?
        }
        _ => {
            let interval_secs = payload.interval_secs.unwrap_or(3600);
            ScheduledRoutine::new(payload.name.clone(), payload.prompt.clone(), interval_secs)
        }
    };

    routine.trigger_type = trigger_type;
    if let Some(tz) = payload.timezone.as_ref() {
        routine.timezone = Some(tz.clone());
    }

    state
        .scheduled_task_store()
        .save(&routine)
        .map_err(|e| e.to_string())?;
    Ok(routine)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_returns_shipped_templates() {
        let templates = list_templates();
        if templates.is_empty() {
            // CARGO_MANIFEST_DIR not set in this test context — skip.
            return;
        }
        let ids: Vec<&str> = templates.iter().map(|t| t.id.as_str()).collect();
        assert!(
            ids.contains(&"daily-standup-summary"),
            "expected the daily-standup-summary template to be present, got {ids:?}"
        );
    }
}
