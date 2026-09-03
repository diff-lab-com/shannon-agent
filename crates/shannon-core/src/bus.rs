//! # Unified in-process event bus (§4.8, W3-1)
//!
//! One bus carries every in-process Shannon event — the engine's query-event
//! stream (mirrored into the L0 session log), permission decisions, and hook
//! firings — so process-internal distribution and persistence share **one
//! vocabulary**: [`SessionEventKind`] / [`SessionEventBody`] from
//! `shannon-types` (vocabulary v1 is frozen; this module invents no kind
//! names).
//!
//! # Dispatch modes
//!
//! | Mode        | Semantics |
//! |-------------|-----------|
//! | [`DispatchMode::Emit`]      | Fan-out to every matching subscriber in registration order, synchronously. The subscriber snapshot is taken under the registry lock and invoked outside it, so a handler may subscribe/unsubscribe/publish without deadlocking. |
//! | [`DispatchMode::Serial`]    | Like Emit, but the whole handler run happens under the bus's dispatch mutex: concurrent producer threads can never interleave two events' handler executions. The L0 mirror uses this mode so `events.jsonl` order equals broadcast order even under multi-threaded production. |
//! | [`DispatchMode::Parallel`]  | Every matching subscriber runs on its own tokio task; the call joins all of them. Without a runtime context it degrades to sequential inline execution. |
//! | [`DispatchMode::Waterfall`] | Ordered guard chain ("`next()` chain"): each node observes/mutates a typed context and either continues or halts. First halt short-circuits. Typed pipelines are created with [`EventBus::guard_pipeline`] and used for the tool pre-execute guards (permission node first, PreToolUse hooks second). |
//!
//! # What rides the bus
//!
//! - [`BusInput::Event`] — a durable event: kind + payload straight from the
//!   frozen L0 vocabulary (`body.kind()` must equal `kind`; the constructor
//!   enforces it). Optional `subtopic` refines routing within one kind (e.g.
//!   hook triggers use [`SessionEventKind::Custom`] with subtopic
//!   `"PreToolUse"`, `"PostToolUse"`, …).
//! - [`BusInput::Coalesce`] — write-side inputs that end up *inside* another
//!   event instead of being standalone kinds (the per-step token/cost triple
//!   folded into the closing `turn/end`, the bare-token fallback, the turn
//!   boundary itself). The vocabulary deliberately has no usage kind; these
//!   arms carry the fold directives to the L0 subscriber only.
//!
//! Subscribers that do not care about coalescing simply match only the
//! `Event` arm.
//!
//! # Registration guards (RAII)
//!
//! Every registration returns a guard whose `Drop` unregisters (verify
//! standard ④): a dropped [`RegistrationGuard`] or [`NodeGuard`] silently
//! stops receiving/running. Guards hold a [`std::sync::Weak`] back-reference,
//! so owning guards never keeps a bus alive.
//!
//! # Permission decisions into L0
//!
//! Both decision sources publish through here (route (b) agreed in §4.9):
//! the [`super::query_engine::guard_nodes::PermissionGateNode`] verdicts,
//! and plugin-manifest gate outcomes routed through the process-wide sink
//! installed by [`install_decision_sink`] (see
//! `crate::plugin::permissions::emit_decision`). They persist as
//! [`SessionEventKind::PermissionDecision`] rows via the same built-in L0
//! subscriber as every other event.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock, Weak};

use async_trait::async_trait;
use shannon_types::session_event::{
    CustomPayload, PermissionDecisionPayload, SessionEventBody, SessionEventKind, TokenUsage,
};

// ============================================================================
// Reserved namespaces for `custom` payloads
// ============================================================================

/// Namespace of the hook-trigger events published by the engine loop. It sits
/// under the reserved internal prefix: trigger rows are routing topics (a
/// trigger is not "a hook fired"), so the L0 subscriber skips them, and audit
/// trails are written as explicit `hook/fired` bodies instead.
pub const NS_HOOK_TRIGGER: &str = "shannon.internal.hooks";

/// Reserved namespace prefix the L0 subscriber never persists. Producers that
/// only want routing (not a durable row) must namespace under this prefix.
const INTERNAL_NS_PREFIXES: [&str; 1] = ["shannon.internal."];

/// True when a `custom` body should be skipped by durable subscribers.
pub fn is_routing_only(payload: &CustomPayload) -> bool {
    payload.namespace.starts_with(INTERNAL_NS_PREFIXES[0])
}

// ============================================================================
// Bus event
// ============================================================================

/// One event in flight on the bus: frozen-vocabulary payload plus routing
/// metadata.
#[derive(Debug, Clone)]
pub struct BusEvent {
    /// Wire kind (derived from `body`; kept as a field for cheap matching).
    pub kind: SessionEventKind,
    /// Optional routing refinement inside one kind (hook type name, …).
    pub subtopic: Option<Arc<str>>,
    /// The unified payload — exactly what persists to L0.
    pub body: SessionEventBody,
    /// Stable producer tag for diagnostics (`origin = "engine-stream"` etc.).
    pub origin: &'static str,
}

impl BusEvent {
    /// Build an event, deriving `kind` from the body (single source of truth).
    pub fn new(body: SessionEventBody) -> Self {
        Self {
            kind: body.kind(),
            subtopic: None,
            body,
            origin: "unattributed",
        }
    }

    /// Set the routing subtopic.
    #[must_use]
    pub fn with_subtopic(mut self, subtopic: impl Into<String>) -> Self {
        self.subtopic = Some(Arc::from(subtopic.into().as_str()));
        self
    }

    /// Set the producer tag.
    #[must_use]
    pub fn with_origin(mut self, origin: &'static str) -> Self {
        self.origin = origin;
        self
    }

    /// True if a wildcard-durable subscriber should skip this event.
    ///
    /// Currently only routing-only `custom` rows are excluded.
    #[must_use]
    pub fn is_routing_only(&self) -> bool {
        match &self.body {
            SessionEventBody::Custom(p) => is_routing_only(p),
            _ => false,
        }
    }
}

/// Write-side inputs that contribute to persistence without being standalone
/// kinds — see the module docs.
#[derive(Debug, Clone)]
pub enum CoalesceInput {
    /// Per-step token/cost triple; the L0 writer folds it into the closing
    /// `turn/end`.
    StepUsage(TokenUsage),
    /// Bare output-token count observed at a step boundary (fallback when no
    /// full triple was seen).
    BareTokens(u64),
    /// The user-visible round ended with `reason`; the L0 writer closes the
    /// open turn exactly once.
    TurnBoundary {
        /// Why the turn ended (`TurnEndPayload::REASON_*`).
        reason: String,
        /// Error text for a failed turn.
        error: Option<String>,
    },
}

/// Everything that can flow through [`EventBus::dispatch`].
#[derive(Debug, Clone)]
pub enum BusInput {
    /// A durable, vocabulary-typed event.
    Event(BusEvent),
    /// A fold directive consumed by the L0 subscriber (and ignored by most).
    Coalesce(CoalesceInput),
}

impl From<BusEvent> for BusInput {
    fn from(event: BusEvent) -> Self {
        BusInput::Event(event)
    }
}

// ============================================================================
// Subscription
// ============================================================================

/// Topic selector for a subscription.
#[derive(Debug, Clone)]
pub struct TopicFilter {
    /// `None` = all kinds.
    kinds: Option<Vec<SessionEventKind>>,
    /// Exact subtopic requirement; `None` matches regardless of subtopic.
    subtopic: Option<String>,
    /// Whether [`BusInput::Coalesce`] fold directives are delivered too.
    /// Kind-scoped subscriptions default to `false` (folds are write-side
    /// directives, not events); the wildcard `all()` delivers them because
    /// the built-in L0 subscriber depends on them.
    include_folds: bool,
}

impl TopicFilter {
    /// Match every topic including fold directives.
    pub fn all() -> Self {
        Self {
            kinds: None,
            subtopic: None,
            include_folds: true,
        }
    }

    /// Match exactly these kinds (no fold directives).
    pub fn kinds(kinds: impl IntoIterator<Item = SessionEventKind>) -> Self {
        Self {
            kinds: Some(kinds.into_iter().collect()),
            subtopic: None,
            include_folds: false,
        }
    }

    /// Match one kind regardless of subtopic.
    pub fn kind(kind: SessionEventKind) -> Self {
        Self::kinds([kind])
    }

    /// Require an exact subtopic in addition to the kind filter.
    #[must_use]
    pub fn with_subtopic(mut self, subtopic: impl Into<String>) -> Self {
        self.subtopic = Some(subtopic.into());
        self
    }

    /// Also deliver [`BusInput::Coalesce`] directives to this (kind-scoped)
    /// subscription.
    #[must_use]
    pub fn with_fold_inputs(mut self) -> Self {
        self.include_folds = true;
        self
    }

    fn matches(&self, input: &BusInput) -> bool {
        let BusInput::Event(event) = input else {
            return self.include_folds;
        };
        if let Some(kinds) = &self.kinds
            && !kinds.contains(&event.kind)
        {
            return false;
        }
        match (&self.subtopic, &event.subtopic) {
            (Some(want), Some(have)) => &**have == want,
            (Some(_), None) => false,
            (None, _) => true,
        }
    }
}

/// Subscriber identity used for ordering and RAII removal.
pub type SubscriberId = u64;

/// Anything that can receive bus inputs.
pub trait BusSubscriber: Send + Sync {
    /// Called once per matching dispatch. Handlers run synchronously except
    /// under [`DispatchMode::Parallel`]; keep them cheap.
    fn on_input(&self, input: &BusInput);
}

/// Adapter so plain closures can subscribe (they only ever see events they
/// filtered for, but receive the full [`BusInput`] enum).
struct FnSubscriber<F>(F);

impl<F> BusSubscriber for FnSubscriber<F>
where
    F: Fn(&BusInput) + Send + Sync,
{
    fn on_input(&self, input: &BusInput) {
        (self.0)(input);
    }
}

// ============================================================================
// Guard pipelines (Waterfall mode)
// ============================================================================

/// Control-flow result of one waterfall step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// Pass control to the next node (or finish the chain).
    Continue,
    /// Stop the chain here; later nodes do not run.
    Halt,
}

/// One node of a typed guard pipeline (Waterfall semantics).
///
/// Nodes see the shared context in registration order and may mutate it
/// (e.g. rewrite tool input) before the next node runs — the "`next()` chain"
/// from plan §4.8.
#[async_trait]
pub trait GuardNode<S>: Send + Sync {
    /// Observe/mutate the context. Return [`Flow::Halt`] to veto the chain.
    async fn guard(&self, ctx: &mut S) -> Flow;
}

/// Outcome of running a guard chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardOutcome {
    /// Every registered node ran.
    Completed(usize),
    /// Node at `index` (0-based registration order) halted the chain;
    /// `node` is its registration label for diagnostics.
    Halted { index: usize, node: &'static str },
}

type AnyMap = HashMap<(TypeId, &'static str), Arc<dyn Any + Send + Sync>>;

/// Typed guard pipeline shared state. Stored in the bus's erased map behind
/// `(TypeId::<S>, key)` and handed out as clones of `GuardChain<S>`.
struct GuardChain<S> {
    nodes: RwLock<Vec<ChainEntry<S>>>,
    next_id: std::sync::atomic::AtomicU64,
}

struct ChainEntry<S> {
    id: u64,
    label: &'static str,
    node: Arc<dyn GuardNode<S>>,
}

/// Handle for registering nodes on / running one named pipeline.
pub struct PipelineHandle<S> {
    chain: Arc<GuardChain<S>>,
}

impl<S> Clone for PipelineHandle<S> {
    fn clone(&self) -> Self {
        Self {
            chain: self.chain.clone(),
        }
    }
}

impl<S: Send> PipelineHandle<S> {
    /// Register a node; the returned guard unregisters on drop.
    pub fn add_node(&self, label: &'static str, node: Arc<dyn GuardNode<S>>) -> NodeGuard<S> {
        let id = self
            .chain
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.chain
            .nodes
            .write()
            .expect("guard chain lock")
            .push(ChainEntry { id, label, node });
        NodeGuard {
            id,
            chain: Arc::downgrade(&self.chain),
            _marker: std::marker::PhantomData,
        }
    }

    /// Run every node in registration order; stop at the first `Halt`.
    pub async fn run(&self, ctx: &mut S) -> GuardOutcome {
        // Snapshot then release the lock: nodes may register further nodes.
        let snapshot: Vec<(Arc<dyn GuardNode<S>>, &'static str)> = {
            let nodes = self.chain.nodes.read().expect("guard chain lock");
            nodes
                .iter()
                .map(|e| (Arc::clone(&e.node), e.label))
                .collect()
        };
        for (index, (node, label)) in snapshot.iter().enumerate() {
            if node.guard(ctx).await == Flow::Halt {
                return GuardOutcome::Halted { index, node: label };
            }
        }
        GuardOutcome::Completed(snapshot.len())
    }

    /// Number of currently registered nodes (introspection/tests).
    pub fn len(&self) -> usize {
        self.chain.nodes.read().expect("guard chain lock").len()
    }

    /// True when no node is registered.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// RAII node registration. Dropping it unlinks the node from its chain.
pub struct NodeGuard<S> {
    id: u64,
    chain: Weak<GuardChain<S>>,
    _marker: std::marker::PhantomData<fn() -> S>,
}

impl<S> Drop for NodeGuard<S> {
    fn drop(&mut self) {
        if let Some(chain) = self.chain.upgrade() {
            chain
                .nodes
                .write()
                .expect("guard chain lock")
                .retain(|e| e.id != self.id);
        }
    }
}

// ============================================================================
// Registration guard for subscribers
// ============================================================================

/// RAII subscription. Dropping it removes the subscriber from the bus
/// (standard ④: after Drop no further events are delivered).
pub struct RegistrationGuard {
    id: SubscriberId,
    bus: Weak<EventBusInner>,
}

impl std::fmt::Debug for RegistrationGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegistrationGuard")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl Drop for RegistrationGuard {
    fn drop(&mut self) {
        if let Some(inner) = self.bus.upgrade() {
            inner
                .registry
                .write()
                .expect("bus registry lock")
                .remove(&self.id);
        }
    }
}

#[derive(Clone)]
struct RegistryEntry {
    filter: TopicFilter,
    subscriber: Arc<dyn BusSubscriber>,
}

struct EventBusInner {
    registry: RwLock<HashMap<SubscriberId, RegistryEntry>>,
    next_id: AtomicCounter,
    /// Serializes whole-handler-run per event across producer threads
    /// ([`DispatchMode::Serial`]).
    serial_lock: Mutex<()>,
    pipelines: Mutex<AnyMap>,
}

type AtomicCounter = std::sync::atomic::AtomicU64;

// ============================================================================
// The bus
// ============================================================================

/// Dispatch-mode parameter of [`EventBus::dispatch`] (plan §4.8 naming:
/// `Emit | Waterfall | Parallel | Serial`). Waterfall runs through the typed
/// guard pipelines ([`EventBus::guard_pipeline`]); passing it here is a
/// documented no-op equal to Emit because the untyped envelope carries no
/// context to thread through the chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchMode {
    /// Synchronous fan-out in registration order.
    Emit,
    /// Totally ordered fan-out across producing threads.
    Serial,
    /// Concurrent handlers joined by the caller (`dispatch_parallel`).
    Parallel,
    /// Ordered guard chain over a typed context (pipelines). See
    /// [`EventBus::guard_pipeline`]; as a raw-dispatch argument this behaves
    /// like Emit.
    Waterfall,
}

/// The unified in-process event bus. Clone cheaply through `Arc` via
/// [`EventBus::shared`], or hand out registrations and keep the owner alive.
#[derive(Clone)]
pub struct EventBus {
    inner: Arc<EventBusInner>,
}

impl std::fmt::Debug for EventBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let guard = self.inner.registry.read().expect("bus registry lock");
        f.debug_struct("EventBus")
            .field("subscribers", &guard.len())
            .finish()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    /// Create a bus.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(EventBusInner {
                registry: RwLock::new(HashMap::new()),
                next_id: AtomicCounter::new(0),
                serial_lock: Mutex::new(()),
                pipelines: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// Shared handle for cloning into tasks.
    pub fn shared(&self) -> Self {
        self.clone()
    }

    /// Subscribe with a filter; returns the RAII guard.
    pub fn subscribe(
        &self,
        filter: TopicFilter,
        subscriber: Arc<dyn BusSubscriber>,
    ) -> RegistrationGuard {
        let id = self
            .inner
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.inner
            .registry
            .write()
            .expect("bus registry lock")
            .insert(id, RegistryEntry { filter, subscriber });
        RegistrationGuard {
            id,
            bus: Arc::downgrade(&self.inner),
        }
    }

    /// Subscribe a closure; returns the RAII guard.
    pub fn subscribe_fn<F>(&self, filter: TopicFilter, on_input: F) -> RegistrationGuard
    where
        F: Fn(&BusInput) + Send + Sync + 'static,
    {
        self.subscribe(filter, Arc::new(FnSubscriber(on_input)))
    }

    /// Number of live subscribers (all filters).
    pub fn subscriber_count(&self) -> usize {
        self.inner.registry.read().expect("bus registry lock").len()
    }

    /// True when some subscription might deliver events of `kind`.
    ///
    /// Cheap probe so eager producers (hook triggers) can skip building
    /// payloads nobody will consume.
    pub fn has_kind_subscription(&self, kind: SessionEventKind) -> bool {
        self.inner
            .registry
            .read()
            .expect("bus registry lock")
            .values()
            .any(|entry| match &entry.filter.kinds {
                None => true,
                Some(kinds) => kinds.contains(&kind),
            })
    }

    /// Get (or lazily create) the named typed guard pipeline.
    ///
    /// Pipelines are keyed by `(S::TypeId, name)`; one instance exists per
    /// key and is shared by all callers.
    pub fn guard_pipeline<S: Send + 'static>(&self, name: &'static str) -> PipelineHandle<S> {
        let key = (TypeId::of::<S>(), name);
        let mut map = self.inner.pipelines.lock().expect("pipeline map lock");
        let handle = Arc::clone(map.entry(key).or_insert_with(|| {
            let chain: Arc<GuardChain<S>> = Arc::new(GuardChain {
                nodes: RwLock::new(Vec::new()),
                next_id: std::sync::atomic::AtomicU64::new(0),
            });
            Arc::new(PipelineHandle { chain }) as Arc<dyn Any + Send + Sync>
        }))
        .downcast::<PipelineHandle<S>>()
        .expect("pipeline stored under its own TypeId");
        (*handle).clone()
    }

    /// Snapshot the matching subscribers in registration (id ascending)
    /// order, releasing the registry lock before invocation.
    fn snapshot(&self, input: &BusInput) -> Vec<Arc<dyn BusSubscriber>> {
        let registry = self.inner.registry.read().expect("bus registry lock");
        let mut entries: Vec<_> = registry
            .iter()
            .filter(|(_, entry)| entry.filter.matches(input))
            .collect();
        entries.sort_by_key(|(id, _)| **id);
        entries
            .into_iter()
            .map(|(_, e)| e.subscriber.clone())
            .collect()
    }

    /// Deliver one input under the given mode.
    pub fn dispatch(&self, input: BusInput, mode: DispatchMode) {
        match mode {
            DispatchMode::Emit | DispatchMode::Waterfall => self.dispatch_emit(input),
            DispatchMode::Serial => self.dispatch_serial(input),
            DispatchMode::Parallel => {
                // Sync bridge into parallel mode: best effort without runtime.
                if tokio::runtime::Handle::try_current().is_ok() {
                    let bus = self.clone();
                    tokio::spawn(async move {
                        bus.dispatch_parallel(input).await;
                    });
                } else {
                    self.dispatch_emit(input);
                }
            }
        }
    }

    /// Emit: synchronous ordered fan-out, handler errors propagate naturally
    /// to the producer context (same contract as direct callbacks today).
    fn dispatch_emit(&self, input: BusInput) {
        for subscriber in self.snapshot(&input) {
            subscriber.on_input(&input);
        }
    }

    /// Serial: identical delivery with a cross-thread total-order guarantee
    /// (one event's handlers complete before another's begin).
    fn dispatch_serial(&self, input: BusInput) {
        let _guard = self.inner.serial_lock.lock().expect("serial lock");
        for subscriber in self.snapshot(&input) {
            subscriber.on_input(&input);
        }
    }

    /// Serial delivery of a **batch** of inputs back to back.
    ///
    /// Used where a single engine event expands into several bus inputs
    /// (`Failed` → error row + turn boundary): the L0 subscriber must never
    /// observe a foreign event interleaved between them. Built-in
    /// subscribers are synchronous, so each element's handler run is still
    /// serialized exactly like [`DispatchMode::Serial`].
    pub fn dispatch_serial_batch<I: IntoIterator<Item = BusInput>>(&self, inputs: I) {
        for input in inputs {
            self.dispatch_serial(input);
        }
    }

    /// Parallel: run each matching handler on its own task and join.
    pub async fn dispatch_parallel(&self, input: BusInput) {
        let subscribers = self.snapshot(&input);
        if tokio::runtime::Handle::try_current().is_err() || subscribers.is_empty() {
            for subscriber in subscribers {
                subscriber.on_input(&input);
            }
            return;
        }
        let input = Arc::new(input);
        let mut handles = Vec::with_capacity(subscribers.len());
        for subscriber in subscribers {
            handles.push(tokio::spawn({
                let input = Arc::clone(&input);
                async move {
                    subscriber.on_input(&input);
                }
            }));
        }
        for handle in handles {
            // A panicking handler must not take the dispatch down.
            let _ = handle.await;
        }
    }
}

// ============================================================================
// Convenience constructors for the shared producers (§4.8 consumers)
// ============================================================================

/// Build the `custom`-kind trigger event for a hook firing request.
///
/// `trigger_type` is the [`crate::HookEventType`](shannon_engine::hooks::HookEventType)
/// wire name (`PreToolUse`, `PostToolUse`, …); `data` carries whatever that
/// type needs (`tool_name`, `input`, `output`, `prompt`, …). Trigger events
/// are routing-only: the L0 subscriber does not persist them.
pub fn hook_trigger_event(trigger_type: &str, data: serde_json::Value) -> BusEvent {
    BusEvent::new(SessionEventBody::Custom(CustomPayload {
        // Routing-only namespace (reserved prefix): skipped by durable
        // subscribers; see [`is_routing_only`].
        namespace: NS_HOOK_TRIGGER.to_string(),
        data: serde_json::json!({
            "type": trigger_type,
            "payload": data,
        }),
    }))
    .with_subtopic(trigger_type)
    .with_origin("hook-hub")
}

/// Build a durable `permission/decision` event from an already-shaped
/// vocabulary payload.
pub fn permission_decision_event(payload: PermissionDecisionPayload) -> BusEvent {
    BusEvent::new(SessionEventBody::PermissionDecision(payload)).with_origin("permission")
}

/// Build a durable `hook/fired` audit row.
pub fn hook_fired_event(payload: shannon_types::session_event::HookFiredPayload) -> BusEvent {
    BusEvent::new(SessionEventBody::HookFired(payload)).with_origin("hooks-audit")
}

// ============================================================================
// Plugin-decision sink (route (b), §4.9 + §4.8)
// ============================================================================}

/// One plugin-gate decision forwarded into the bus world.
#[derive(Debug, Clone)]
pub struct PluginDecisionFrame {
    /// Plugin whose manifest was consulted.
    pub plugin: String,
    /// Requested permission face (`execute_commands`, …).
    pub required: String,
    /// Full declared allow-set (empty when undeclared → no frame ever).
    pub declared: Vec<String>,
    /// Enforcement point (`spawn`, `transport`, `mcp_tools`, …).
    pub point: String,
    /// True when the gate admitted the operation.
    pub allowed: bool,
}

type DecisionSink = Arc<dyn Fn(&PluginDecisionFrame) + Send + Sync>;

static DECISION_SINK: std::sync::OnceLock<Mutex<Option<DecisionSink>>> = std::sync::OnceLock::new();

/// Install the process-wide receiver for plugin-gate decisions. The query
/// layer installs a closure that republishes frames onto the current session
/// bus so both decision sources land in L0 with one schema.
///
/// Limitation: a process hosts many sessions concurrently in principle; the
/// sink targets the most recently installing session (single-active-session
/// desktop flows). Gate denial tracing remains the authoritative feed.
pub fn install_decision_sink(sink: DecisionSink) {
    let cell = DECISION_SINK.get_or_init(|| Mutex::new(None));
    *cell.lock().expect("decision sink lock") = Some(sink);
}

/// Test helper: remove a previously installed sink.
#[cfg(test)]
pub(crate) fn clear_decision_sink_for_test() {
    if let Some(cell) = DECISION_SINK.get() {
        *cell.lock().expect("decision sink lock") = None;
    }
}

/// Forward one plugin-gate decision to the installed sink, if any.
pub(crate) fn broadcast_plugin_decision(frame: PluginDecisionFrame) {
    if let Some(cell) = DECISION_SINK.get() {
        let guard = cell.lock().expect("decision sink lock");
        if let Some(sink) = guard.as_ref() {
            sink(&frame);
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use shannon_types::session_event::ErrorPayload;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn err_body(message: &str) -> SessionEventBody {
        SessionEventBody::Error(ErrorPayload {
            category: "test".into(),
            message: message.into(),
            detail: None,
        })
    }

    fn err_input(message: &str) -> BusInput {
        BusInput::Event(BusEvent::new(err_body(message)))
    }

    /// Collector counting received inputs, cloneable across subscribers.
    #[derive(Default)]
    struct Counter(AtomicUsize);

    impl Counter {
        fn get(&self) -> usize {
            self.0.load(Ordering::SeqCst)
        }
    }

    #[test]
    fn emit_fans_out_in_registration_order_and_filters_by_kind() {
        let bus = EventBus::new();
        let counter = Arc::new(Counter::default());
        let seen: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));

        // First subscriber: all error-kind events.
        let c2 = counter.clone();
        let s2 = seen.clone();
        let guard_first = bus.subscribe_fn(TopicFilter::kind(SessionEventKind::Error), move |_| {
            c2.0.fetch_add(1, Ordering::SeqCst);
            s2.lock().unwrap().push("first");
        });

        // Second subscriber: listens to a different kind — must NOT fire.
        let c_other = Arc::new(Counter::default());
        let g_other_c = c_other.clone();
        let guard_other =
            bus.subscribe_fn(TopicFilter::kind(SessionEventKind::TodoWrite), move |_| {
                g_other_c.0.fetch_add(1, Ordering::SeqCst);
            });

        let c3 = counter.clone();
        let s3 = seen.clone();
        let guard_last = bus.subscribe_fn(TopicFilter::kind(SessionEventKind::Error), move |_| {
            c3.0.fetch_add(1, Ordering::SeqCst);
            s3.lock().unwrap().push("second");
        });

        bus.dispatch(err_input("boom"), DispatchMode::Emit);

        assert_eq!(counter.get(), 2, "only Error-kind subscribers ran");
        assert_eq!(
            *seen.lock().unwrap(),
            vec!["first", "second"],
            "registration order"
        );
        assert_eq!(c_other.get(), 0);

        // RAII: dropping the middle + other guards stops their delivery.
        drop(guard_other);
        drop(guard_first);
        drop(guard_last);
        assert_eq!(bus.subscriber_count(), 0, "guards unregister on Drop");

        let after_drop = Arc::new(Counter::default());
        let ad = after_drop.clone();
        let guard_new = bus.subscribe_fn(TopicFilter::all(), move |_| {
            ad.0.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(after_drop.get(), 0, "no delivery yet");
        bus.dispatch(err_input("again"), DispatchMode::Emit);
        assert_eq!(after_drop.get(), 1);
        drop(guard_new);
        bus.dispatch(err_input("post-drop"), DispatchMode::Emit);
        assert_eq!(after_drop.get(), 1, "dropped guard receives nothing (④)");
    }

    #[tokio::test]
    async fn parallel_mode_runs_all_handlers_concurrently() {
        let bus = EventBus::new();
        let hits = Arc::new(Counter::default());
        // Hold the guards for the scope — dropping one unregisters it.
        let mut guards = Vec::new();
        for _ in 0..8 {
            let h = hits.clone();
            guards.push(bus.subscribe_fn(TopicFilter::all(), move |_| {
                h.0.fetch_add(1, Ordering::SeqCst);
            }));
        }
        bus.dispatch_parallel(err_input("x")).await;
        assert_eq!(hits.get(), 8, "every concurrent handler joined");
    }

    #[test]
    fn serial_mode_is_callable_off_runtime() {
        let bus = EventBus::new();
        let hits = Arc::new(Counter::default());
        let h = hits.clone();
        let _keep = bus.subscribe_fn(TopicFilter::all(), move |_| {
            h.0.fetch_add(1, Ordering::SeqCst);
        });
        bus.dispatch(err_input("serial"), DispatchMode::Serial);
        assert_eq!(hits.get(), 1);
    }

    #[tokio::test]
    async fn guard_pipeline_waterfall_short_circuits_on_first_halt() {
        struct Append(char);

        #[async_trait]
        impl GuardNode<Vec<char>> for Append {
            async fn guard(&self, ctx: &mut Vec<char>) -> Flow {
                ctx.push(self.0);
                if self.0 == 'h' {
                    Flow::Halt
                } else {
                    Flow::Continue
                }
            }
        }

        let bus = EventBus::new();
        let pipe: PipelineHandle<Vec<char>> = bus.guard_pipeline("tool/pre-execute-test");
        let a = pipe.add_node("append-a", Arc::new(Append('a')));
        let h = pipe.add_node("halt-h", Arc::new(Append('h')));
        let z = pipe.add_node("append-z", Arc::new(Append('z')));

        let mut ctx = Vec::new();
        let outcome = pipe.run(&mut ctx).await;

        assert_eq!(
            outcome,
            GuardOutcome::Halted {
                index: 1,
                node: "halt-h"
            },
            "first halt wins"
        );
        assert_eq!(ctx, vec!['a', 'h'], "nodes after the halt never ran");

        // RAII node removal: drop the halting node and the chain completes.
        drop(h);
        assert_eq!(pipe.len(), 2);
        let mut ctx2 = Vec::new();
        let outcome2 = pipe.run(&mut ctx2).await;
        assert_eq!(outcome2, GuardOutcome::Completed(2));
        assert_eq!(ctx2, vec!['a', 'z']);

        drop(a);
        drop(z);
        assert_eq!(pipe.len(), 0, "node guards unlink on Drop");
    }

    #[test]
    fn subtopic_filter_routes_within_one_kind() {
        let bus = EventBus::new();
        let pre = Arc::new(Counter::default());
        let post = Arc::new(Counter::default());
        let p = pre.clone();
        let _pre_guard = bus.subscribe_fn(
            TopicFilter::kind(SessionEventKind::Custom).with_subtopic("PreToolUse"),
            move |_| {
                p.0.fetch_add(1, Ordering::SeqCst);
            },
        );
        let q = post.clone();
        let _post_guard = bus.subscribe_fn(
            TopicFilter::kind(SessionEventKind::Custom).with_subtopic("PostToolUse"),
            move |_| {
                q.0.fetch_add(1, Ordering::SeqCst);
            },
        );

        bus.dispatch(
            BusInput::Event(hook_trigger_event("PreToolUse", serde_json::json!({}))),
            DispatchMode::Emit,
        );
        assert_eq!(pre.get(), 1);
        assert_eq!(post.get(), 0);

        bus.dispatch(
            BusInput::Event(hook_trigger_event("PostToolUse", serde_json::json!({}))),
            DispatchMode::Emit,
        );
        assert_eq!(pre.get(), 1);
        assert_eq!(post.get(), 1);
    }

    #[test]
    fn hook_triggers_are_marked_routing_only() {
        let event = hook_trigger_event("UserPromptSubmit", serde_json::json!({"prompt": "hi"}));
        assert!(event.is_routing_only(), "hook triggers are routing topics");
        assert_eq!(
            event.subtopic.as_ref().map(|s| &**s),
            Some("UserPromptSubmit"),
            "subtopic carries the hook type name"
        );
        // Non-custom events stay durable.
        assert!(!BusEvent::new(err_body("x")).is_routing_only());
    }

    #[test]
    fn permission_decision_builder_carries_vocabulary_payload() {
        let event = permission_decision_event(PermissionDecisionPayload {
            tool_name: Some("Bash".into()),
            request: None,
            decision: "allow".into(),
            reason: Some("rule allow-bash-ls".into()),
            mode: Some("auto".into()),
        });
        assert_eq!(event.kind, SessionEventKind::PermissionDecision);
        assert_eq!(event.body.kind(), SessionEventKind::PermissionDecision);
    }

    #[test]
    fn decision_sink_broadcast_reaches_installed_receiver_only_when_set() {
        clear_decision_sink_for_test();
        // No sink installed: broadcast is a silent no-op.
        broadcast_plugin_decision(PluginDecisionFrame {
            plugin: "p".into(),
            required: "network".into(),
            declared: vec![],
            point: "transport".to_string(),
            allowed: false,
        });

        let got = Arc::new(Mutex::new(Vec::new()));
        let g = got.clone();
        install_decision_sink(Arc::new(move |frame: &PluginDecisionFrame| {
            g.lock()
                .unwrap()
                .push((frame.plugin.clone(), frame.allowed));
        }));
        broadcast_plugin_decision(PluginDecisionFrame {
            plugin: "acme".into(),
            required: "mcp_tools".into(),
            declared: vec!["mcp_tools".into()],
            point: "mcp_tools".to_string(),
            allowed: true,
        });
        assert_eq!(
            *got.lock().unwrap(),
            vec![("acme".to_string(), true)],
            "installed sink receives gate decisions"
        );
        clear_decision_sink_for_test();
    }

    #[test]
    fn bus_event_derives_kind_from_body_single_source_of_truth() {
        let event = BusEvent::new(err_body("mismatch probe"));
        assert_eq!(event.kind, SessionEventKind::Error);
        assert_eq!(event.origin, "unattributed");
    }
}
