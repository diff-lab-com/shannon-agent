/**
 * GENERATED FILE — DO NOT EDIT BY HAND.
 *
 * Source of truth: `shannon-api-protocol` (Rust).
 * Generator:     `cargo run -p shannon-api-protocol --bin gen-ts`.
 * Protocol:      v0.6.0
 *
 * Field names, casing, and discriminated unions match the serde-derived
 * Rust types 1:1. Anything that mutates must mutate there first and be
 * regenerated here. The runtime lives in this same folder and consumes
 * these types directly; only the contract is generated.
 */

export interface QueryRequest {
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

export interface WsClientMessageQuery {
  type: "query";
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

export type WsClientMessage =
  | WsClientMessageQuery
  | WsClientMessageClear
  | WsClientMessageInfo
  | WsClientMessageCancel;

export interface WsServerMessageText {
  type: "text";
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
  description: string;
  diff_preview?: string | null;
  is_destructive: boolean;
  request_id: string;
  tool_input: unknown;
  tool_name: string;
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

export type WsServerMessage =
  | WsServerMessageText
  | WsServerMessageToolUse
  | WsServerMessageToolResult
  | WsServerMessageUsage
  | WsServerMessageCompleted
  | WsServerMessageFailed
  | WsServerMessageCancelled
  | WsServerMessageApprovalRequest
  | WsServerMessageSessionInfo
  | WsServerMessageError;

export const PROTOCOL_VERSION = "0.6.0" as const;
