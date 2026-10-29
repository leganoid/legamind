// Обёртки над invoke(): единственное место, где UI общается с Rust.
import { invoke } from "@tauri-apps/api/core";
import type {
  AppInfo,
  ChatMessageRow,
  ChatStart,
  AppendResult,
  CandidateCheck,
  CheckResult,
  CommandResult,
  ContextUsage,
  DatasetRow,
  DebateDetail,
  DebateRow,
  DebateStartResult,
  EditStart,
  ExtractedItem,
  FeedbackRow,
  FinetuneStatus,
  LessonRow,
  MemoryRow,
  ModelInfo,
  ProviderInput,
  ProviderModelRow,
  ProviderView,
  RegenStart,
  Session,
  Skill,
  SkillInput,
  SourceRow,
  WebConfigView,
  WebResult,
  WebTestResult,
} from "./types";

export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),
  openDataDir: () => invoke<void>("open_data_dir"),

  sessionsList: () => invoke<Session[]>("sessions_list"),
  sessionsCreate: (modelId: string, title?: string) =>
    invoke<Session>("sessions_create", { title: title ?? null, modelId }),
  sessionsRename: (id: string, title: string) => invoke<void>("sessions_rename", { id, title }),
  sessionsDelete: (id: string) => invoke<void>("sessions_delete", { id }),
  messagesList: (sessionId: string) => invoke<ChatMessageRow[]>("messages_list", { sessionId }),

  modelsList: () => invoke<ModelInfo[]>("models_list"),
  modelsLoad: (id: string) => invoke<{ loaded: boolean; load_seconds: number | null }>("models_load", { id }),
  modelsUnload: (id: string) => invoke<void>("models_unload", { id }),
  engineRestart: () => invoke<number>("engine_restart"),

  chatSend: (sessionId: string, content: string, modelId: string) =>
    invoke<ChatStart>("chat_send", { sessionId, content, modelId }),
  chatStop: (requestId: string) => invoke<void>("chat_stop", { requestId }),
  chatRegenerate: (sessionId: string, modelId: string) =>
    invoke<RegenStart>("chat_regenerate", { sessionId, modelId }),
  chatEditAndResend: (sessionId: string, messageId: string, content: string, modelId: string) =>
    invoke<EditStart>("chat_edit_and_resend", { sessionId, messageId, content, modelId }),
  contextUsage: (sessionId: string, modelId: string) =>
    invoke<ContextUsage>("context_usage", { sessionId, modelId }),

  chatCommand: (sessionId: string, modelId: string, input: string) =>
    invoke<CommandResult>("chat_command", { sessionId, modelId, input }),

  skillsList: () => invoke<Skill[]>("skills_list"),
  skillSave: (input: SkillInput) => invoke<Skill>("skill_save", { input }),
  skillDelete: (id: string) => invoke<void>("skill_delete", { id }),
  skillSetEnabled: (id: string, enabled: boolean) =>
    invoke<void>("skill_set_enabled", { id, enabled }),

  lessonsList: () => invoke<LessonRow[]>("lessons_list"),
  lessonAdd: (text: string, source?: string | null) =>
    invoke<LessonRow>("lesson_add", { text, source: source ?? null }),
  lessonDelete: (id: string) => invoke<void>("lesson_delete", { id }),
  lessonSetEnabled: (id: string, enabled: boolean) =>
    invoke<void>("lesson_set_enabled", { id, enabled }),
  feedbackSet: (messageId: string, sessionId: string, rating: number, note?: string | null) =>
    invoke<FeedbackRow>("feedback_set", { messageId, sessionId, rating, note: note ?? null }),

  datasetsList: () => invoke<DatasetRow[]>("datasets_list"),
  datasetGenerate: (req: {
    name: string;
    teacher_model: string;
    source_text?: string | null;
    session_id?: string | null;
    count?: number;
  }) => invoke<DatasetRow>("dataset_generate", { req }),
  datasetDelete: (id: string) => invoke<void>("dataset_delete", { id }),
  datasetCreate: (name: string) => invoke<DatasetRow>("dataset_create", { name }),
  datasetAppend: (datasetId: string, items: { instruction: string; answer: string }[]) =>
    invoke<AppendResult>("dataset_append", { datasetId, items }),
  sessionExtract: (sessionId: string, teacherModel: string) =>
    invoke<ExtractedItem[]>("session_extract", { sessionId, teacherModel }),
  candidateCheck: (modelId: string, instruction: string, answer: string) =>
    invoke<CandidateCheck>("candidate_check", { modelId, instruction, answer }),
  finetuneStatus: () => invoke<FinetuneStatus>("finetune_status"),

  webConfigGet: () => invoke<WebConfigView>("web_config_get"),
  webConfigSet: (input: {
    enabled?: boolean;
    provider?: string;
    searxng_url?: string;
    google_cx?: string;
    tavily_key?: string;
    brave_key?: string;
    google_key?: string;
  }) => invoke<WebConfigView>("web_config_set", { input }),
  webSearch: (query: string, limit?: number) =>
    invoke<WebResult[]>("web_search", { query, limit: limit ?? 5 }),
  webTest: () => invoke<WebTestResult>("web_test"),
  webFetchPage: (url: string) => invoke<string>("web_fetch_page", { url }),
  sourcesList: (sessionId: string) => invoke<SourceRow[]>("sources_list", { sessionId }),

  pickFolder: () => invoke<string | null>("pick_folder"),
  sessionSetWorkdir: (sessionId: string, path: string | null, create = true) =>
    invoke<Session>("session_set_workdir", { sessionId, path, create }),
  openPath: (path: string) => invoke<void>("open_path", { path }),
  sessionSetPolicy: (
    sessionId: string,
    patch: { mode?: string; trust?: string; reasoning?: string },
  ) => invoke<Session>("session_set_policy", { sessionId, ...patch }),

  providersList: () => invoke<ProviderView[]>("providers_list"),
  providerSave: (input: ProviderInput) => invoke<ProviderView>("provider_save", { input }),
  providerCheck: (input: ProviderInput) => invoke<CheckResult>("provider_check", { input }),
  providerRefresh: (id: string) => invoke<CheckResult>("provider_refresh", { id }),
  providerSetEnabled: (id: string, enabled: boolean) =>
    invoke<void>("provider_set_enabled", { id, enabled }),
  providerDelete: (id: string) => invoke<void>("provider_delete", { id }),
  providerModels: (id: string) => invoke<ProviderModelRow[]>("provider_models", { id }),

  memoryAdd: (kind: string, text: string, sessionId?: string | null) =>
    invoke<MemoryRow>("memory_add", { kind, text, sessionId: sessionId ?? null }),
  memoriesList: (limit?: number) => invoke<MemoryRow[]>("memories_list", { limit: limit ?? null }),
  memorySearch: (query: string, limit?: number) =>
    invoke<MemoryRow[]>("memory_search", { query, limit: limit ?? null }),
  memoryDelete: (id: string) => invoke<void>("memory_delete", { id }),
  memoryContextPreview: (query: string) => invoke<string | null>("memory_context_preview", { query }),

  debateStart: (req: {
    task: string;
    participants: string[];
    rounds: number;
    judge: string | null;
    max_tokens: number;
  }) => invoke<DebateStartResult>("debate_start", { req }),
  debateStop: (debateId: string) => invoke<void>("debate_stop", { debateId }),
  debatesList: (limit?: number) => invoke<DebateRow[]>("debates_list", { limit: limit ?? null }),
  debateGet: (id: string) => invoke<DebateDetail>("debate_get", { id }),
  debateExportMd: (id: string) => invoke<string>("debate_export_md", { id }),
};
