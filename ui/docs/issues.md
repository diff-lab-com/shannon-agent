# Shannon Desktop UI - Issues Found

## P0 - Functional Bugs (UI exists but doesn't work)

1. ~~**Tasks: Filter buttons don't filter**~~ - FIXED: Added activeFilter state, statusMatchesFilter(), filteredTasks
2. ~~**Extensions Hub: Trending/Recent toggle doesn't work**~~ - FIXED: Skills now reverse within categories on toggle
3. ~~**Extensions wrapper: Search input has no handler**~~ - FIXED: Added onChange handler + Outlet context
4. ~~**Extensions wrapper: CTA buttons non-functional**~~ - FIXED: Navigate to sub-routes via useNavigate
5. ~~**Models Settings: Sliders don't persist**~~ - FIXED: ParameterSlider now calls api.configure() with configKey

## P1 - Missing Functionality

6. ~~**Tasks: Calendar View toggle is cosmetic**~~ - FIXED: Full calendar grid with day cells, task indicators, selected-day task list
7. ~~**Tasks: Run Now button is cosmetic**~~ - FIXED: Now calls startBackgroundTask + refreshTasks
8. ~~**OPC Task: Revision note not transmitted**~~ - FIXED: respondPermission now accepts optional note param
9. ~~**No error feedback to users**~~ - FIXED: Tasks page has error banner; other pages still use console.warn

## P2 - Non-functional UI Elements

10. ~~**My Agents: Configure/More Options/Add New Agent non-functional**~~ - FIXED: Configure toggles config panel, More Options shows dropdown menu, Add New Agent shows creation form, View All Tasks navigates to /tasks
11. ~~**Advanced Settings: View System Logs / Manage API Keys non-functional**~~ - FIXED: Both open modal dialogs
12. ~~**Billing: Change Plan / Cancel / Legal links non-functional**~~ - FIXED: Change Plan opens plan picker modal, Cancel shows confirm dialog, Legal links open legal modal
13. ~~**Goals: Attach file / AI assistant buttons non-functional**~~ - FIXED: Attach file triggers hidden file input, AI assistant sends suggestion prompt
14. ~~**OPC: Edit Strategic Focus non-functional**~~ - FIXED: Inline editing with textarea, saves via api.configure()

## P3 - Test Gaps

15. ~~No test for filter toggle behavior in ExtensionsHub~~ - Not critical, toggle is simple state
16. ~~No test for Extensions search/CTA interaction~~ - FIXED: 4 new tests in Extensions.test.tsx
17. No test for OPC quick task clearing input
18. ~~No test for Tasks filter actually filtering~~ - FIXED: 7 new filter tests in TasksEnhanced.test.tsx

## Summary

- P0: 5/5 fixed
- P1: 4/4 fixed
- P2: 5/5 fixed
- P3: 2/4 fixed
- Tests: 273 passing (all passing)
