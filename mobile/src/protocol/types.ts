// TypeScript mirror of Pui's v1 Remote Protocol (`src/remote/mod.rs`).

export type OriginPlatform = 'desktop' | 'android' | 'ios' | 'web' | 'cli';

export interface SessionOrigin {
  platform: OriginPlatform;
  device?: string;
  client?: string;
  created_at?: number;
}

export interface SavedHost {
  id: string;
  name: string;
  host: string;
  port: number;
  tls: boolean;
  /** Stored separately in SecureStore when persisted; hydrated in memory. */
  token: string;
  /** Phase 1 = 'direct' (Tailscale/LAN). Phase 2 = 'cloud_relay' (E2EE relay). */
  transport: 'direct' | 'cloud_relay';
  relayUrl?: string;
}

export interface PermissionModeOption {
  key: string;
  label: string;
}

export interface HostInfo {
  protocol_version: number;
  pui_version: string;
  host_name: string;
  tailscale_ip: string | null;
  active_path: string | null;
  projects: string[];
  permission_modes: PermissionModeOption[];
}

export interface SessionSummary {
  path: string;
  title: string;
  project: string;
  project_name: string;
  cwd: string;
  status: string;
  busy: boolean;
  unread: boolean;
  has_pending_dialog: boolean;
  model: string;
  modified_ms: number;
  created_ms?: number | null;
  origin: SessionOrigin;
  parent_session?: string | null;
  pinned?: boolean;
  is_active: boolean;
}

export interface MediaAttachment {
  mime: string;
  data: string;
  file?: {
    name: string;
    path: string;
  } | null;
}

export interface InlineVisual {
  title: string;
  html: string;
  height?: number;
}

export interface ToolRecord {
  id: string;
  name: string;
  args: string;
  output: string;
  failed: boolean;
  finished: boolean;
  path: string;
  command: string;
  diff: string;
  added: number;
  removed: number;
  duration_ms?: number | null;
  code: string;
  children: ToolRecord[];
  images: MediaAttachment[];
  visual?: InlineVisual | null;
  open_target?: string | null;
  open_queued?: boolean;
}

export interface MessageRecord {
  role: string;
  text: string;
  thinking: string;
  provider: string;
  model: string;
  error?: string | null;
  tools: ToolRecord[];
  images: MediaAttachment[];
  timestamp: number;
  cost: number;
  effort: string;
  revision: number;
}

export interface PendingMessageRecord {
  id: string;
  text: string;
  status: string;
}

export interface WaitingMessageItem {
  index: number;
  text: string;
  mode: 'steer' | 'queue' | string;
  image_count: number;
}

export interface SlashCommandItem {
  name: string;
  description: string;
  source?: string;
}

export interface ModelItem {
  id: string;
  name: string;
  provider: string;
  context_window: number;
  input_cost: number;
  output_cost: number;
}

export interface ArtifactItem {
  id: string;
  kind: 'visual_html' | 'file_html' | 'image' | 'file' | string;
  title: string;
  html?: string;
  height?: number;
  path?: string;
  mime?: string;
  data_base64?: string;
  message_index: number;
}

export interface SessionDetail {
  path: string;
  title: string;
  cwd: string;
  project: string;
  status: string;
  busy: boolean;
  ready: boolean;
  model: string;
  model_name: string;
  provider: string;
  thinking: string;
  levels: string[];
  permission_mode: string;
  cost: number;
  context_tokens?: number | null;
  context_window: number;
  error?: string | null;
  notice?: string | null;
  origin: SessionOrigin;
  messages: MessageRecord[];
  pending_messages: PendingMessageRecord[];
  waiting_messages?: WaitingMessageItem[];
  dialogs: Record<string, unknown>[];
  commands: SlashCommandItem[];
  models: ModelItem[];
  artifacts: ArtifactItem[];
  revision: number;
}

export interface FileMentionItem {
  path: string;
  name: string;
}

export interface TreeEntry {
  name: string;
  path: string;
  is_dir: boolean;
  size: number;
  is_image: boolean;
  is_html: boolean;
}

export interface TreeResponse {
  cwd: string;
  dir: string;
  entries: TreeEntry[];
  truncated: boolean;
}

export interface FileContentResponse {
  path: string;
  size: number;
  truncated: boolean;
  is_html: boolean;
  is_image: boolean;
  is_binary: boolean;
  mime: string;
  content: string | null;
}

export interface GitChangeItem {
  path: string;
  status: string;
  added: string;
  removed: string;
}

export interface UsageWindow {
  label: string;
  used: number;
  resets?: number | null;
  detail?: string | null;
}

export interface ProviderUsage {
  id: string;
  name: string;
  plan?: string | null;
  windows: UsageWindow[];
  models: UsageWindow[];
  notes: string[];
  error?: string | null;
}

export interface AccountUsage {
  provider: string;
  spend: number;
  max?: number | null;
  reset?: string | null;
  duration: string;
  month: string;
  month_spend: number;
  requests: number;
  total: number;
}

export interface UsageResponse {
  data?: AccountUsage | null;
  providers: ProviderUsage[];
  unsupported: string[];
  litellm_error?: string | null;
}

export interface GitChangesResponse {
  cwd: string;
  branch: string;
  is_git: boolean;
  error: string | null;
  files: GitChangeItem[];
}

export interface NotificationEvent {
  id: string;
  kind: 'finished' | 'approval' | 'question' | string;
  path: string;
  title: string;
  body: string;
  timestamp: number;
  dialog_id?: string | null;
  dialog_method?: string | null;
}

export type WsServerEvent =
  | {
      type: 'hello';
      protocol_version: number;
      pui_version: string;
      host_name: string;
      active_path: string | null;
    }
  | {
      type: 'sessions_updated';
      revision: number;
      sessions: SessionSummary[];
    }
  | {
      type: 'session_updated';
      revision: number;
      path: string;
      detail: SessionDetail;
    }
  | {
      type: 'notification';
      notification: NotificationEvent;
    }
  | {
      type: 'pong';
      ts?: number;
    };
