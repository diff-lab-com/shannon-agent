//! Team coordination system prompt injection.
//!
//! When team tools (TeamCreate, TeamTaskCreate, etc.) are registered,
//! this module generates coordination instructions so the LLM knows
//! how to create teams, delegate tasks, and communicate.

/// Returns the team coordination system prompt if team tools are detected.
///
/// Check is based on whether `TeamCreate` or `TeamTaskCreate` is in the tool list.
pub fn team_coordination_prompt(tool_names: &[String]) -> Option<String> {
    let has_team_tools = tool_names.iter().any(|n| {
        matches!(
            n.as_str(),
            "TeamCreate" | "TeamTaskCreate" | "TeamTaskUpdate" | "TeamTaskList"
        )
    });

    if !has_team_tools {
        return None;
    }

    Some(TEAM_COORDINATION_PROMPT.to_string())
}

/// Returns the teammate-specific instructions for spawned agents.
pub fn teammate_instructions() -> &'static str {
    TEAMMATE_INSTRUCTIONS
}

const TEAM_COORDINATION_PROMPT: &str = "\
# Team Coordination

You can coordinate multiple AI agents as a team to work on tasks in parallel.

## Creating a Team

Use `TeamCreate` to create a new team:
```
TeamCreate({ team_name: \"my-team\", description: \"Working on feature X\" })
```

## Breaking Down Work

Use `TeamTaskCreate` to create tasks with dependencies:
```
TeamTaskCreate({
  subject: \"Implement auth middleware\",
  description: \"Add JWT validation to all /api routes\",
  owner: \"\",           // leave empty for self-claim
  addBlocks: [\"3\"],    // IDs of tasks that must complete first
})
```

## Spawning Teammates

Use the `Agent` tool with `team_name` to spawn a teammate:
```
Agent({
  prompt: \"You are a backend specialist. Check TaskList, claim an available task, and execute it.\",
  team_name: \"my-team\",
  name: \"backend-dev\",
  subagent_type: \"general-purpose\"
})
```

## Communication

Use `SendMessage` to coordinate:
- DM a teammate: `SendMessage({ to: \"backend-dev\", message: \"API schema is ready\" })`
- Broadcast: `SendMessage({ to: \"*\", message: \"All tasks unblocked\" })`

## Task Lifecycle

1. Create tasks with `TeamTaskCreate` (set dependencies with `addBlocks`)
2. Teammates self-claim tasks via `TaskUpdate` (set `owner` to their name)
3. Mark completed: `TeamTaskUpdate({ taskId: \"1\", status: \"completed\" })`
4. Check progress: `TeamTaskList()`

## Best Practices

- Create all tasks upfront with proper dependencies before spawning teammates
- Each teammate should check `TaskList` and self-claim the next available task
- Use specific, actionable task subjects
- Prefer small, well-scoped tasks over large ambiguous ones
- Send progress updates via `SendMessage` when completing milestones
- When all tasks are done, use `TeamDelete` to clean up

## As a Teammate

If you were spawned as a teammate:
1. Read `TaskList` to find available (unblocked, unowned) tasks
2. Claim a task with `TaskUpdate` (set yourself as owner)
3. Execute the task
4. Mark it completed
5. Repeat until no tasks remain
6. Send an idle message to the team lead
";

const TEAMMATE_INSTRUCTIONS: &str = "\
You are a teammate in a team. Follow this workflow:
1. Call TaskList to find available tasks.
2. Claim an unblocked, unowned task with TaskUpdate (set owner to your name).
3. Execute the task.
4. Mark it completed with TaskUpdate (status: \"completed\").
5. Repeat from step 1.
6. If no tasks remain, send an idle message via SendMessage to the team lead.
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_prompt_without_team_tools() {
        let tools = vec!["Bash".to_string(), "Read".to_string(), "Write".to_string()];
        assert!(team_coordination_prompt(&tools).is_none());
    }

    #[test]
    fn test_prompt_with_team_create() {
        let tools = vec![
            "Bash".to_string(),
            "TeamCreate".to_string(),
            "Read".to_string(),
        ];
        let prompt = team_coordination_prompt(&tools);
        assert!(prompt.is_some());
        let text = prompt.unwrap();
        assert!(text.contains("TeamCreate"));
        assert!(text.contains("TeamTaskCreate"));
        assert!(text.contains("SendMessage"));
        assert!(text.contains("TaskList"));
    }

    #[test]
    fn test_prompt_with_team_task_create() {
        let tools = vec!["TeamTaskCreate".to_string()];
        let prompt = team_coordination_prompt(&tools);
        assert!(prompt.is_some());
    }

    #[test]
    fn test_prompt_with_team_task_update() {
        let tools = vec!["TeamTaskUpdate".to_string()];
        let prompt = team_coordination_prompt(&tools);
        assert!(prompt.is_some());
    }

    #[test]
    fn test_prompt_content_completeness() {
        let tools = vec!["TeamCreate".to_string()];
        let prompt = team_coordination_prompt(&tools).unwrap();
        // Must cover all key concepts
        assert!(prompt.contains("Creating a Team"));
        assert!(prompt.contains("Breaking Down Work"));
        assert!(prompt.contains("Spawning Teammates"));
        assert!(prompt.contains("Communication"));
        assert!(prompt.contains("Task Lifecycle"));
        assert!(prompt.contains("Best Practices"));
        assert!(prompt.contains("As a Teammate"));
    }

    #[test]
    fn test_teammate_instructions() {
        let instructions = teammate_instructions();
        assert!(instructions.contains("TaskList"));
        assert!(instructions.contains("TaskUpdate"));
        assert!(instructions.contains("completed"));
        assert!(instructions.contains("SendMessage"));
    }

    #[test]
    fn test_empty_tool_list() {
        assert!(team_coordination_prompt(&[]).is_none());
    }

    #[test]
    fn test_team_task_list_triggers() {
        let tools = vec!["TeamTaskList".to_string()];
        assert!(team_coordination_prompt(&tools).is_some());
    }

    #[test]
    fn test_mixed_tools_with_team() {
        let tools = vec![
            "Bash".to_string(),
            "Read".to_string(),
            "TeamCreate".to_string(),
            "Write".to_string(),
        ];
        let prompt = team_coordination_prompt(&tools);
        assert!(prompt.is_some());
    }

    #[test]
    fn test_duplicate_team_tools() {
        let tools = vec!["TeamCreate".to_string(), "TeamCreate".to_string()];
        let prompt = team_coordination_prompt(&tools);
        assert!(prompt.is_some());
        // Should only return one prompt, not concatenated
        assert_eq!(prompt.unwrap().matches("Team Coordination").count(), 1);
    }
}
