// Общие типы данных, приходящих из Rust-команд.

export interface Session {
  id: string;
  title: string;
  model_id: string;
  created_at: string;
  updated_at: string;
}

export interface ChatMessageRow {
  id: string;
  session_id: string;
  role: "user" | "assistant" | "system" | string;
  content: string;
  model_id: string | null;
  tokens: number | null;
  created_at: string;
}

export interface ModelInfo {
  id: string;
  label: string;
  provider: string;
  kind: "local" | "cloud";
  device: string | null;
  size_gb: number | null;
  loaded: boolean;
  thinks: boolean;
}

export interface ProviderView {
  id: string;
  kind: "openai" | "anthropic";
  label: string;
  base_url: string;
  enabled: boolean;
  timeout_s: number;
  has_key: boolean;
  key_mask: string | null;
  models_count: number;
  created_at: string;
}

export interface ProviderModelRow {
  provider_id: string;
  model_id: string;
  label: string | null;
  context: number | null;
  updated_at: string;
}

export interface ProviderInput {
  id?: string | null;
  kind: string;
  label: string;
  base_url: string;
  api_key?: string | null;
  timeout_s?: number | null;
}

export interface CheckResult {
  ok: boolean;
  message: string;
  models: string[];
}

export interface MemoryRow {
  id: string;
  kind: "fact" | "episode" | "preference" | string;
  text: string;
  source_session: string | null;
  created_at: string;
}

export interface DebateRow {
  id: string;
  task: string;
  participants: string;
  rounds: number;
  judge: string | null;
  verdict: string | null;
  final_answer: string | null;
  created_at: string;
}

export interface DebateTurnRow {
  id: string;
  debate_id: string;
  round: number;
  role: "proposal" | "critique" | "vote" | "judge" | string;
  model_id: string;
  content: string;
  tokens: number | null;
  seconds: number | null;
  created_at: string;
}

export interface DebateDetail {
  debate: DebateRow;
  turns: DebateTurnRow[];
}

export interface ScoreEntry {
  label: string;
  score: number;
}

export interface AppInfo {
  version: string;
  db_path: string;
  data_dir: string;
  sidecar_port: number | null;
  engine_status: string;
  python_path: string;
}

export interface ChatStart {
  request_id: string;
  message: ChatMessageRow;
}

export interface ChatDeltaPayload {
  request_id: string;
  session_id: string;
  delta: string;
}

export interface ChatDonePayload {
  request_id: string;
  session_id: string;
  message: ChatMessageRow;
  tokens: number | null;
  seconds: number;
  stopped: boolean;
}

export interface ChatErrorPayload {
  request_id: string;
  session_id: string;
  error: string;
}

export interface DebateTurnPayload {
  debate_id: string;
  turn: DebateTurnRow;
}

export interface DebateDeltaPayload {
  debate_id: string;
  model_id: string;
  round: number;
  delta: string;
}

export interface DebateDonePayload {
  debate_id: string;
  verdict: string;
  final_answer: string;
  scores: ScoreEntry[];
}

export interface DebateErrorPayload {
  debate_id: string;
  error: string;
}

export interface DebateStartResult {
  debate_id: string;
}
