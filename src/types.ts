// Общие типы данных, приходящих из Rust-команд.

export interface Session {
  id: string;
  title: string;
  model_id: string;
  created_at: string;
  updated_at: string;
  workdir: string | null;
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
  context: number | null;
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
  context: ContextUsage | null;
}

export interface ContextUsage {
  used: number;
  limit: number;
  reserve: number;
  dropped: number;
  included: number;
}

export interface ChatStatusPayload {
  request_id: string;
  session_id: string;
  phase: string;
  label: string | null;
  device: string | null;
  seconds: number | null;
}

export interface RegenStart {
  request_id: string;
  removed_ids: string[];
}

export interface EditStart {
  request_id: string;
  removed_ids: string[];
  message: ChatMessageRow;
}

export interface Skill {
  id: string;
  name: string;
  description: string | null;
  role: string | null;
  guidelines: string | null;
  output_format: string | null;
  examples: string | null;
  constraints: string | null;
  triggers: string | null;
  enabled: boolean;
  builtin: boolean;
  created_at: string;
  preview: string;
}

export interface SkillInput {
  id?: string | null;
  name: string;
  description?: string | null;
  role?: string | null;
  guidelines?: string | null;
  output_format?: string | null;
  examples?: string | null;
  constraints?: string | null;
  triggers?: string | null;
  enabled?: boolean | null;
}

export interface CommandResult {
  kind: "notice" | "clear" | "compact" | string;
  text: string;
  removed_ids: string[];
  message: ChatMessageRow | null;
}

export interface LessonRow {
  id: string;
  text: string;
  source: string | null;
  enabled: boolean;
  created_at: string;
}

export interface FeedbackRow {
  id: string;
  message_id: string;
  session_id: string;
  rating: number;
  note: string | null;
  created_at: string;
}

export interface DatasetRow {
  id: string;
  name: string;
  path: string;
  teacher_model: string | null;
  examples: number;
  created_at: string;
}

export interface DatasetProgress {
  dataset_id: string;
  phase: "generating" | "done" | "error" | string;
  done: number;
  total: number;
  examples: number;
  error: string | null;
}

export interface FinetuneStatus {
  python: string;
  packages: Record<string, boolean>;
  missing: string[];
  device: string;
  hf_models: string[];
  model_root: string;
  ready: boolean;
  note: string;
}

export interface ExtractedItem {
  id: string;
  instruction: string;
  answer: string;
  duplicate_of: string | null;
  duplicate_score: number;
  warnings: string[];
}

export interface CandidateCheck {
  model_answer: string;
  coverage: number;
  jaccard: number;
  verdict: "already" | "partial" | "new" | string;
}

export interface AppendResult {
  added: number;
  skipped: number;
  total: number;
}

export interface WebConfigView {
  enabled: boolean;
  provider: string;
  searxng_url: string;
  has_tavily: boolean;
  has_brave: boolean;
}

export interface WebResult {
  title: string;
  url: string;
  snippet: string;
}

export interface WebTestResult {
  provider: string;
  results: WebResult[];
  error: string | null;
}

export interface SourceRow {
  id: string;
  message_id: string;
  session_id: string;
  title: string | null;
  url: string;
  snippet: string | null;
  created_at: string;
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
