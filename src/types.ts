// Общие типы данных, приходящих из Rust-команд.

export interface Session {
  id: string;
  title: string;
  model_id: string;
  created_at: string;
  updated_at: string;
  workdir: string | null;
  mode: "chat" | "plan" | "build" | "review";
  trust: "read_only" | "workspace" | "full";
  reasoning: string | null;
  pinned: boolean;
}

export interface MessageHitRow {
  session_id: string;
  session_title: string;
  message_id: string;
  role: string;
  snippet: string;
  created_at: string;
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
  efforts: string[];
  role: "chat" | "embedding" | "rerank" | "image";
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
  role: "chat" | "embedding" | "rerank" | "image";
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
  quality: string;
  searxng_url: string;
  google_cx: string;
  has_tavily: boolean;
  has_brave: boolean;
  has_google: boolean;
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

export interface ToolApproval {
  approval_id: string;
  request_id: string;
  session_id: string;
  kind: "write" | "command" | "delete" | "move" | string;
  path: string | null;
  relative: string | null;
  diff: string | null;
  is_new: boolean;
  bytes: number;
  command: string | null;
  cwd: string | null;
  target: string | null;
  warning: string | null;
}

export interface ToolEventPayload {
  request_id: string;
  session_id: string;
  tool: string;
  path: string | null;
  summary: string | null;
  detail: string | null;
  ok: boolean;
}

export interface ToolRunRow {
  id: string;
  session_id: string;
  request_id: string | null;
  tool: string;
  path: string | null;
  ok: boolean;
  summary: string | null;
  output: string | null;
  created_at: string;
}

export interface RagStats {
  chunks: number;
  files: number;
}

export interface BackupInfo {
  last_auto: string | null;
  dir: string;
  auto_count: number;
}

export interface RagIndexedPayload {
  chunks: number;
  files: number;
}

export interface ToolActivity {
  id: string;
  tool: string;
  path: string | null;
  summary: string | null;
  detail: string | null;
  ok: boolean;
  status: "running" | "pending" | "done";
}

export interface MessageToolRow {
  message_id: string;
  tool: string;
  path: string | null;
  ok: boolean;
  summary: string | null;
  output: string | null;
}

export interface LocalModelRow {
  id: string;
  label: string;
  path: string;
  device: string;
  max_tokens: number;
  thinks: boolean;
  size_gb: number;
}

export interface LogTail {
  name: string;
  path: string;
  text: string;
  size: number;
}

export interface EngineStats {
  loaded: string[];
  rss_mb: number | null;
  models: number;
}

export interface SelfCheckItem {
  id: string;
  label: string;
  status: "ok" | "warn" | "fail";
  detail: string;
}

export interface ModelInspection {
  path: string;
  label: string;
  size_gb: number;
  has_xml: boolean;
  has_bin: boolean;
  has_tokenizer: boolean;
  context_tokens: number | null;
}

export interface HfFile {
  path: string;
  size: number;
}

export interface HfPreview {
  repo: string;
  files: HfFile[];
  total_bytes: number;
  ir_ready: boolean;
  tokenizer_ready: boolean;
  label: string;
}

export interface ModelDownloadEvent {
  repo: string;
  file: string;
  downloaded: number;
  total: number;
  file_index: number;
  file_count: number;
  done: boolean;
}

export interface EmbedStats {
  embedded: number;
  total: number;
  model: string;
}

export interface ModelUsage {
  model_id: string;
  messages: number;
  tokens: number;
}

export interface PickedAttachment {
  name: string;
  path: string;
  kind: string;
  content: string;
  data_url: string | null;
  size: number;
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
