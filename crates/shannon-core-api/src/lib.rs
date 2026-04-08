//! # Shannon Core API
//!
//! LLM API client and related services for the Shannon system.
//!
//! This crate provides:
//! - Multi-provider LLM API client with streaming support
//! - API request/response management
//! - Usage tracking and statistics
//! - VCR (Virtual Cassette Recorder) for testing

pub mod api;
pub mod api_services;
pub mod vcr;

// Re-exports for convenience
pub use api::{
    ApiError,
    ContentBlock,
    ContentDelta,
    ImageSource,
    LlmClient,
    LlmClientConfig,
    LlmProvider,
    Message,
    MessageContent,
    MessageRequest,
    MessageResponse,
    MessageStream,
    StreamEvent,
    ToolDefinition,
    Usage,
};

pub use api_services::{
    ApiManager,
    ApiRequest,
    ApiResponse,
    ApiServiceError,
    ModelUsage,
    RateLimitInfo,
    UsageStats,
    UsageTracker,
};

pub use vcr::{
    Vcr,
    VcrConfig,
    VcrError,
    VcrRecording,
};
