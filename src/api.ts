// Обёртки над invoke(): единственное место, где UI общается с Rust.
import { invoke } from "@tauri-apps/api/core";
import type {
  AppInfo,
  ChatMessageRow,
  ChatStart,
  CheckResult,
  DebateDetail,
  DebateRow,
  DebateStartResult,
  MemoryRow,
  ModelInfo,
  ProviderInput,
  ProviderModelRow,
  ProviderView,
  Session,
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
