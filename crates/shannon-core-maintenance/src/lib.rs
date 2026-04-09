//! Shannon Core Maintenance
//!
//! Background tasks, housekeeping, rate limiting, and policy management.

pub mod activity_manager;
pub mod ai_limits;
pub mod away_summary;
pub mod housekeeping;
pub mod policy_limits;
pub mod prevent_sleep;
pub mod rate_limit;
pub mod rate_limit_messages;

// Activity tracking
pub use activity_manager::{
    Activity, ActivityManager, ActivityStatus, ActivityError,
};

// AI limits tracking
pub use ai_limits::{
    AiLimitsTracker, AiLimitType, AiUsageRecord, LimitStatus,
};

// Away summary generation
pub use away_summary::{
    AwaySummaryGenerator, ConversationMessage,
};

// Background housekeeping
pub use housekeeping::{
    Housekeeper, HousekeepingConfig, HousekeepingTask, HousekeepingError,
    TaskResult, TempFileCleanupTask, CacheRefreshTask, OldSessionPruneTask,
    LogRotationTask,
};

// Policy limits
pub use policy_limits::{
    PolicyLimitsManager, PolicyLimits, PolicyError, PolicyCheckResult,
};

// Sleep prevention (platform-specific functions)
pub use prevent_sleep::{
    is_preventing_sleep, start_prevent_sleep, stop_prevent_sleep,
    force_stop_prevent_sleep,
};

// Rate limiting
pub use rate_limit::{
    RateLimiter, RateLimitConfig, RateLimitResult, TokenBucket,
    ExponentialBackoff,
};

// Rate limit messages
pub use rate_limit_messages::RateLimitMessageBuilder;
