// Обёртки над invoke(): единственное место, где UI общается с Rust.
import { invoke } from "@tauri-apps/api/core";
import type {
  AppInfo,
  BackupInfo,
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
  EngineStats,
  EmbedStats,
  ExtractedItem,
  FeedbackRow,
  FinetuneStatus,
  LessonRow,
  LocalModelRow,
  LogTail,
  ModelInspection,
  HfPreview,
  MemoryRow,
  MessageHitRow,
  MessageToolRow,
  ModelInfo,
  RagConfig,
  ModelUsage,
  ImportResult,
  PickedAttachment,
  ProviderInput,
  ProviderModelRow,
  ProviderView,
  RegenStart,
  Session,
  Skill,
  SkillInput,
  SourceRow,
  ToolRunRow,
  RagStats,
  SelfCheckItem,
  WebConfigView,
  WebResult,
  WebTestResult,
} from "./types";

export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),
  openDataDir: () => invoke<void>("open_data_dir"),

  sessionsList: () => invoke<Session[]>("sessions_list"),
  messagesSearch: (query: string, limit = 20) =>
    invoke<MessageHitRow[]>("messages_search", { query, limit }),
  sessionsCreate: (modelId: string, title?: string) =>
    invoke<Session>("sessions_create", { title: title ?? null, modelId }),
  sessionsRename: (id: string, title: string) => invoke<void>("sessions_rename", { id, title }),
  sessionsDelete: (id: string) => invoke<void>("sessions_delete", { id }),
  sessionPin: (id: string, pinned: boolean) => invoke<void>("session_pin", { id, pinned }),
  sessionsSetFolder: (id: string, folder: string) =>
    invoke<void>("sessions_set_folder", { id, folder }),
  messagesList: (sessionId: string) => invoke<ChatMessageRow[]>("messages_list", { sessionId }),

  modelsList: () => invoke<ModelInfo[]>("models_list"),
  modelsLoad: (id: string) => invoke<{ loaded: boolean; load_seconds: number | null }>("models_load", { id }),
  modelsUnload: (id: string) => invoke<void>("models_unload", { id }),
  engineRestart: () => invoke<number>("engine_restart"),

  chatSend: (
    sessionId: string,
    content: string,
    modelId: string,
    attachments?: {
      name: string;
      path: string;
      content: string;
      data_url?: string | null;
    }[],
  ) => invoke<ChatStart>("chat_send", { sessionId, content, modelId, attachments }),
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
    quality?: string;
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
  toolApprove: (approvalId: string, approve: boolean, remember = false) =>
    invoke<void>("tool_approve", { approvalId, approve, remember }),
  toolRunsList: (sessionId: string, limit = 20) =>
    invoke<ToolRunRow[]>("tool_runs_list", { sessionId, limit }),
  messageTools: (sessionId: string) =>
    invoke<MessageToolRow[]>("message_tools", { sessionId }),
  quickAsk: (prompt: string, modelId: string) =>
    invoke<string>("quick_ask", { prompt, modelId }),
  overlayHide: () => invoke<void>("overlay_hide"),
  usageStats: () => invoke<ModelUsage[]>("usage_stats"),
  importChat: (path: string) => invoke<ImportResult>("import_chat", { path }),
  usagePricesGet: () => invoke<Record<string, number>>("usage_prices_get"),
  usagePricesSet: (prices: Record<string, number>) =>
    invoke<void>("usage_prices_set", { prices }),
  sessionPermissionsClear: (sessionId: string) =>
    invoke<void>("session_permissions_clear", { sessionId }),
  toolUndoLast: (sessionId: string) => invoke<string>("tool_undo_last", { sessionId }),
  ragReindex: (sessionId: string) => invoke<RagStats>("rag_reindex", { sessionId }),
  ragStatus: (sessionId: string) => invoke<RagStats>("rag_status", { sessionId }),
  ragGetEnabled: () => invoke<boolean>("rag_get_enabled"),
  ragSetEnabled: (enabled: boolean) => invoke<boolean>("rag_set_enabled", { enabled }),
  ragConfigGet: () => invoke<RagConfig>("rag_config_get"),
  ragConfigSet: (config: RagConfig) => invoke<RagConfig>("rag_config_set", { config }),
  ragEmbeddingGet: () => invoke<string | null>("rag_embedding_get"),
  ragEmbeddingSet: (model: string | null) => invoke<void>("rag_embedding_set", { model }),
  ragRerankerGet: () => invoke<string | null>("rag_reranker_get"),
  ragRerankerSet: (model: string | null) => invoke<void>("rag_reranker_set", { model }),
  ragEmbeddingStatus: (sessionId: string) =>
    invoke<EmbedStats>("rag_embedding_status", { sessionId }),
  ragEmbedProject: (sessionId: string) =>
    invoke<EmbedStats>("rag_embed_project", { sessionId }),
  dbBackup: (encrypted = false) => invoke<string>("db_backup", { encrypted }),
  dbRestore: () => invoke<string>("db_restore"),
  dbBackupInfo: () => invoke<BackupInfo>("db_backup_info"),
  exportAll: () => invoke<string>("export_all"),
  localModelsList: () => invoke<LocalModelRow[]>("local_models_list"),
  localModelAdd: (input: {
    path: string;
    label: string;
    device?: string;
    max_tokens?: number;
    thinks?: boolean;
  }) => invoke<LocalModelRow>("local_model_add", { input }),
  localModelRemove: (id: string) => invoke<void>("local_model_remove", { id }),
  inspectLocalModel: (path: string) =>
    invoke<ModelInspection>("inspect_local_model", { path }),
  hfModelPreview: (repo: string) => invoke<HfPreview>("hf_model_preview", { repo }),
  hfModelDownload: (repo: string) => invoke<string>("hf_model_download", { repo }),
  logsTail: (name: string, lines = 400) =>
    invoke<LogTail>("logs_tail", { name, lines }),
  engineStats: () => invoke<EngineStats>("engine_stats"),
  selfCheck: () => invoke<SelfCheckItem[]>("self_check"),
  pickTextAttachment: () => invoke<PickedAttachment | null>("pick_text_attachment"),
  watchWorkdir: (sessionId: string) => invoke<string>("watch_workdir", { sessionId }),
  unwatchWorkdir: () => invoke<void>("unwatch_workdir"),

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
  providerModelRole: (providerId: string, modelId: string, role: string) =>
    invoke<void>("provider_model_role", { providerId, modelId, role }),

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
