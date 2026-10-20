// Typed wrappers over the Tauri commands in src-tauri/src/lib.rs.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type ProviderKind = "opencode" | "custom" | "apple";

export interface Settings {
  onboarded: boolean;
  provider: { kind: ProviderKind; base_url: string; score_model: string; summary_model: string };
  defaults: { style: string; preset: string; layout: string; max_clips: number; min_score: number; output_dir: string };
  speech: string;
  language: string;
}

export interface Recommended {
  id: string;
  label: string;
  cents_per_source_hour: number;
  precision_at_5: number;
  note: string;
}

export interface AppleStatus {
  available: boolean;
  os: string;
  locales: string[];
  installed_locales: string[];
  llm_available: boolean;
  llm_status: string;
}

export interface Status {
  settings: Settings;
  has_key: boolean;
  speech_engines: ({ kind: "apple_speech" } | { kind: "whisper"; device: string })[];
  apple: AppleStatus | null;
  whisper_needed: string | null;
  whisper_installed: boolean;
  hardware: { os: string; os_version: string; arch: string; cpu_cores: number; memory_bytes: number | null; gpu: string | null };
  engine_version: string;
  recommended: Recommended[];
  styles: string[];
  presets: string[];
}

export interface Candidate {
  id: string;
  start_ms: number;
  end_ms: number;
  score: number;
  reasons: string[];
  title: string;
  description: string;
  hashtags: string[];
}

export interface ExportedClip {
  rank: number;
  candidate: Candidate;
  mp4: string;
  poster: string;
  srt: string;
  vtt: string;
  json: string;
  render: { frames: number; duration_ms: number; wall_ms: number };
  reframe: { layout: string; switches: number; moves: number } | null;
}

export interface RunResult {
  source: { path: string; duration_ms: number };
  picker: { windows: number; shortlisted: number; scored: number; model_calls: number; errors: string[] };
  clips: ExportedClip[];
  summary_path: string;
}

export interface JobRequest {
  file: string;
  prompt: string | null;
  style: string;
  preset: string;
  layout: string;
  max_clips: number;
  min_score: number;
  output_dir: string;
}

export interface ProgressEvent {
  job: number;
  stage: string;
  fraction: number;
  message: string;
}

export const api = {
  status: () => invoke<Status>("status"),
  diagnostics: () => invoke<Record<string, unknown>>("diagnostics"),
  saveSettings: (settings: Settings) => invoke<void>("save_settings", { settings }),
  setApiKey: (key: string) => invoke<void>("set_api_key", { key }),
  providerModels: (baseUrl: string, key?: string) => invoke<string[]>("provider_models", { baseUrl, key: key ?? null }),
  providerTest: (baseUrl: string, model: string, key?: string) => invoke<number>("provider_test", { baseUrl, model, key: key ?? null }),
  startJob: (req: JobRequest) => invoke<number>("start_job", { req }),
  cancelJob: (job: number) => invoke<void>("cancel_job", { job }),
  reexportClip: (summaryPath: string, rank: number, style: string, preset: string, layout: string) =>
    invoke<ExportedClip>("reexport_clip", { summaryPath, rank, style, preset, layout }),
  loadRun: (summaryPath: string) => invoke<RunResult>("load_run", { summaryPath }),
  downloadSpeechModel: () => invoke<void>("download_speech_model"),
};

export function onJobProgress(f: (e: ProgressEvent) => void): Promise<UnlistenFn> {
  return listen<ProgressEvent>("job-progress", (e) => f(e.payload));
}
export function onJobDone(f: (e: { job: number; result: RunResult }) => void): Promise<UnlistenFn> {
  return listen<{ job: number; result: RunResult }>("job-done", (e) => f(e.payload));
}
export function onJobError(f: (e: { job: number; error: string }) => void): Promise<UnlistenFn> {
  return listen<{ job: number; error: string }>("job-error", (e) => f(e.payload));
}
export function onModelProgress(f: (e: ProgressEvent) => void): Promise<UnlistenFn> {
  return listen<ProgressEvent>("model-progress", (e) => f(e.payload));
}

export const fmtTime = (ms: number) => `${Math.floor(ms / 60000)}:${String(Math.floor(ms / 1000) % 60).padStart(2, "0")}`;
export const OPENCODE_URL = "https://opencode.ai/zen/v1";
export const OPENCODE_SIGNUP = "https://opencode.ai/auth";
