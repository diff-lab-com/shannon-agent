/**
 * GENERATED FILE — DO NOT EDIT BY HAND.
 *
 * Source of truth: `shannon-api-protocol` (Rust).
 * Generator:     `cargo run -p shannon-api-protocol --bin gen-ts`.
 * Protocol:      v0.8.0
 *
 * Field names, casing, and discriminated unions match the serde-derived
 * Rust types 1:1. Anything that mutates must mutate there first and be
 * regenerated here. The runtime lives in this same folder and consumes
 * these types directly; only the contract is generated.
 */

export interface MessageAttachment {
  data: string;
  media_type: string;
  name: string | null;
}

export interface QueryRequest {
  attachments: MessageAttachment[] | null;
  model: string | null;
  prompt: string;
  session_id: string | null;
}

export interface QueryResponse {
  errors: string[];
  model: string;
  session_id: string;
  text: string;
  usage: UsageInfo | null;
}

export interface UsageInfo {
  cost_usd: number;
  input_tokens: number;
  output_tokens: number;
}

export interface HealthResponse {
  status: string;
  version: string;
}

export interface ModelInfo {
  id: string;
  name?: string | null;
  provider: string;
}

export interface ModelsResponse {
  models: ModelInfo[];
}

export interface ToolEntry {
  description: string;
  name: string;
}

export interface ToolsListResponse {
  tools: ToolEntry[];
}

export interface ApprovalRespondRequest {
  choice: ApprovalDecision;
  request_id: string;
}

export type ApprovalDecision =
  | "allow_once"
  | "always_allow"
  | "deny";

export type SseEventName =
  | "completed"
  | "conversation_update"
  | "cost"
  | "error"
  | "failed"
  | "info"
  | "progress"
  | "rate_limit"
  | "started"
  | "text"
  | "thinking"
  | "tool_progress"
  | "tool_use_request"
  | "tool_use_result"
  | "turn_completed"
  | "usage"
  | "warning";

export interface SessionSummary {
  created_at: string;
  preview: string | null;
  session_id: string;
  title: string | null;
  total_input_tokens: number;
  total_output_tokens: number;
  turn_count: number;
  updated_at: string;
}

export interface TranscriptMessage {
  content: string;
  role: string;
  ts: string;
}

export interface AgentRef {
  id: string | null;
  name: string | null;
}

export interface RiskInfo {
  reversible: boolean;
  scope: RiskScope;
}

export type RiskScope =
  | "local"
  | "repo"
  | "system";

export interface WsClientMessageQuery {
  type: "query";
  attachments?: MessageAttachment[] | null;
  model?: string | null;
  prompt: string;
  session_id?: string | null;
}
export interface WsClientMessageClear {
  type: "clear";
}
export interface WsClientMessageInfo {
  type: "info";
}
export interface WsClientMessageCancel {
  type: "cancel";
}
export interface WsClientMessageSessionsList {
  type: "sessions.list";
}
export interface WsClientMessageSessionHistory {
  type: "session.history";
  before?: string | null;
  limit?: number | null;
  session_id: string;
}

export type WsClientMessage =
  | WsClientMessageQuery
  | WsClientMessageClear
  | WsClientMessageInfo
  | WsClientMessageCancel
  | WsClientMessageSessionsList
  | WsClientMessageSessionHistory;

export interface WsServerMessageText {
  type: "text";
  content: string;
}
export interface WsServerMessageThinking {
  type: "thinking";
  content: string;
}
export interface WsServerMessageToolUse {
  type: "tool_use";
  input: unknown;
  name: string;
}
export interface WsServerMessageToolResult {
  type: "tool_result";
  name: string;
  output: string;
}
export interface WsServerMessageUsage {
  type: "usage";
  cost_usd: number;
  input_tokens: number;
  output_tokens: number;
}
export interface WsServerMessageCompleted {
  type: "completed";
  model: string;
}
export interface WsServerMessageFailed {
  type: "failed";
  error: string;
}
export interface WsServerMessageCancelled {
  type: "cancelled";
}
export interface WsServerMessageApprovalRequest {
  type: "approval_request";
  agent?: AgentRef | null;
  description: string;
  diff_preview?: string | null;
  is_destructive: boolean;
  request_id: string;
  risk?: RiskInfo | null;
  tool_input: unknown;
  tool_name: string;
  ts?: number | null;
}
export interface WsServerMessageSessionInfo {
  type: "session_info";
  message_count: number;
  model?: string | null;
  protocol_version?: string | null;
}
export interface WsServerMessageError {
  type: "error";
  message: string;
}
export interface WsServerMessageSessionsSnapshot {
  type: "sessions.snapshot";
  sessions: SessionSummary[];
}
export interface WsServerMessageSessionTranscript {
  type: "session.transcript";
  has_more: boolean;
  messages: TranscriptMessage[];
  session_id: string;
}

export type WsServerMessage =
  | WsServerMessageText
  | WsServerMessageThinking
  | WsServerMessageToolUse
  | WsServerMessageToolResult
  | WsServerMessageUsage
  | WsServerMessageCompleted
  | WsServerMessageFailed
  | WsServerMessageCancelled
  | WsServerMessageApprovalRequest
  | WsServerMessageSessionInfo
  | WsServerMessageError
  | WsServerMessageSessionsSnapshot
  | WsServerMessageSessionTranscript;

export const PROTOCOL_VERSION = "0.8.0" as const;
