import React, { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react';
import { AppState, Platform } from 'react-native';
import * as Device from 'expo-device';
import * as SecureStore from 'expo-secure-store';
import type {
  HostInfo,
  LinkedHostInfo,
  SavedHost,
  SessionDetail,
  SessionOrigin,
  SessionSummary,
  WsServerEvent,
} from '../protocol/types';
import { saveChatDraft } from '../services/drafts';
import { startBackgroundKeepAlive, stopBackgroundKeepAlive } from '../services/keepAlive';
import { getNotificationPermissionStatus, presentAgentNotification } from '../services/notifications';
import type { SharedPayload } from '../services/shareIntent';
import {
  CloudRelayTransport,
  DirectTransport,
  type ConnectionState,
  type PuiTransport,
} from '../transport/transport';
import { shouldPresentNotification } from '../utils/notifications';
import { usePreferences } from './preferencesStore';

const HOSTS_KEY = 'pui_saved_hosts_v1';
const ACTIVE_HOST_KEY = 'pui_active_host_v1';
const DEVICE_NAME_KEY = 'pui_device_name_v1';

interface StoredHostMeta {
  id: string;
  name: string;
  host: string;
  port: number;
  tls: boolean;
  transport: 'direct' | 'cloud_relay';
  relayUrl?: string;
}

function tokenKey(id: string): string {
  return `pui_host_token_${id.replace(/[^a-zA-Z0-9._-]/g, '_')}`;
}

function isCloudHostEntry(host: SavedHost, info?: HostInfo | null): boolean {
  if (info?.pui_version?.toLowerCase().includes('cloud')) return true;
  return host.name.toLowerCase().includes('cloud');
}

/**
 * Uses Android Keystore / iOS Keychain on native devices, with localStorage fallback on Web
 * so `expo start --web` works cleanly without throwing.
 */
async function storageGet(key: string): Promise<string | null> {
  if (Platform.OS === 'web') {
    try {
      return typeof localStorage !== 'undefined' ? localStorage.getItem(key) : null;
    } catch {
      return null;
    }
  }
  return SecureStore.getItemAsync(key);
}

async function storageSet(key: string, value: string): Promise<void> {
  if (Platform.OS === 'web') {
    try {
      if (typeof localStorage !== 'undefined') localStorage.setItem(key, value);
    } catch {
      // ignore
    }
    return;
  }
  await SecureStore.setItemAsync(key, value);
}

async function storageDelete(key: string): Promise<void> {
  if (Platform.OS === 'web') {
    try {
      if (typeof localStorage !== 'undefined') localStorage.removeItem(key);
    } catch {
      // ignore
    }
    return;
  }
  await SecureStore.deleteItemAsync(key);
}

export interface PuiContextValue {
  hosts: SavedHost[];
  activeHost: SavedHost | null;
  connectionState: ConnectionState;
  connectionError: string | null;
  deviceName: string;
  setDeviceName: (name: string) => Promise<void>;
  hostInfo: HostInfo | null;
  /** Unified list of sessions across all paired hosts (Mac + Cloud), tagged with host_id/host_kind. */
  sessions: SessionSummary[];
  detailsByPath: Record<string, SessionDetail>;
  transport: PuiTransport | null;
  getTransportForHost: (hostId?: string | null) => PuiTransport | null;
  getHostInfoForHost: (hostId?: string | null) => HostInfo | null;
  clientOrigin: () => SessionOrigin;
  saveHost: (host: Omit<SavedHost, 'id'> & { id?: string }) => Promise<SavedHost>;
  removeHost: (id: string) => Promise<void>;
  selectHost: (id: string) => Promise<void>;
  retryConnection: () => void;
  refreshSessions: () => Promise<void>;
  loadSessionDetail: (path: string, hostId?: string | null) => Promise<SessionDetail | null>;
  sendMessage: (
    path: string,
    hostId: string | null | undefined,
    input: {
      text: string;
      images?: { data_base64: string; mime: string; name?: string }[];
      mode?: 'queue' | 'steer';
      origin: SessionOrigin;
    },
  ) => Promise<void>;
  removeQueuedMessage: (
    path: string,
    hostId: string | null | undefined,
    index: number,
  ) => Promise<void>;
  setActiveChatPath: (path: string | null) => void;
  pendingShare: SharedPayload | null;
  setPendingShare: (payload: SharedPayload | null) => void;
}

const PuiContext = createContext<PuiContextValue | null>(null);

interface LiveHostEntry {
  sig: string;
  transport: PuiTransport;
}

export function PuiProvider({ children }: { children: React.ReactNode }) {
  const { prefs } = usePreferences();
  const prefsRef = useRef(prefs);
  prefsRef.current = prefs;

  const [hosts, setHosts] = useState<SavedHost[]>([]);
  const hostsRef = useRef<SavedHost[]>([]);
  const [activeHostId, setActiveHostId] = useState<string | null>(null);
  const activeHostIdRef = useRef<string | null>(null);
  activeHostIdRef.current = activeHostId;
  const [deviceName, setDeviceNameState] = useState<string>(
    Device.deviceName ?? Device.modelName ?? (Platform.OS === 'ios' ? 'iPhone' : 'Android Phone'),
  );

  const [stateByHost, setStateByHost] = useState<Record<string, ConnectionState>>({});
  const [errorByHost, setErrorByHost] = useState<Record<string, string | null>>({});
  const [hostInfoByHost, setHostInfoByHost] = useState<Record<string, HostInfo>>({});
  const [sessionsByHost, setSessionsByHost] = useState<Record<string, SessionSummary[]>>({});
  const [detailsByPath, setDetailsByPath] = useState<Record<string, SessionDetail>>({});
  const [transportVersion, setTransportVersion] = useState(0);
  const [pendingShare, setPendingShare] = useState<SharedPayload | null>(null);

  const liveHostsRef = useRef<Map<string, LiveHostEntry>>(new Map());
  const detailsRef = useRef<Record<string, SessionDetail>>({});
  const inFlightDetailsRef = useRef<Map<string, Promise<SessionDetail | null>>>(new Map());
  const prefetchedHostsRef = useRef<Set<string>>(new Set());
  const optimisticSendsRef = useRef<
    Record<
      string,
      { id: number; text: string; mode: 'queue' | 'steer'; image_count: number; baselineMsgCount: number }[]
    >
  >({});
  const nextOptimisticIdRef = useRef(-1);
  const appStateRef = useRef<string>(AppState.currentState || 'active');
  const activeChatPathRef = useRef<string | null>(null);
  const seenNotificationIdsRef = useRef<Set<string>>(new Set());

  const setActiveChatPath = useCallback((path: string | null) => {
    activeChatPathRef.current = path;
  }, []);

  useEffect(() => {
    const sub = AppState.addEventListener('change', (nextState) => {
      appStateRef.current = nextState;
    });
    return () => sub.remove();
  }, []);

  useEffect(() => {
    hostsRef.current = hosts;
  }, [hosts]);

  useEffect(() => {
    detailsRef.current = detailsByPath;
  }, [detailsByPath]);

  /**
   * Merge an incoming server `SessionDetail` without letting a transient empty
   * boot snapshot wipe out already-loaded disk history, and preserve any
   * still-in-flight optimistic sends until the host confirms them.
   */
  const mergeDetail = useCallback((path: string, incoming: SessionDetail): SessionDetail => {
    const prev = detailsRef.current[path];
    const messages =
      incoming.messages.length === 0 && prev && prev.messages.length > 0
        ? prev.messages
        : incoming.messages;
    const artifacts =
      incoming.artifacts.length === 0 &&
      incoming.messages.length === 0 &&
      prev &&
      prev.artifacts.length > 0
        ? prev.artifacts
        : incoming.artifacts;

    const pendingOpt = optimisticSendsRef.current[path];
    let waiting_messages = incoming.waiting_messages ?? [];
    if (pendingOpt && pendingOpt.length > 0) {
      const remaining = pendingOpt.filter((opt) => {
        const trimmed = opt.text.trim();
        const inServerQueue = waiting_messages.some((w) => w.text.trim() === trimmed);
        const inPending = (incoming.pending_messages ?? []).some(
          (p) => p.text.trim() === trimmed,
        );
        const deliveredToTranscript =
          messages.length > opt.baselineMsgCount &&
          messages
            .slice(opt.baselineMsgCount)
            .some((m) => m.role === 'user' && (trimmed === '' || m.text.trim() === trimmed));
        return !inServerQueue && !inPending && !deliveredToTranscript;
      });
      if (remaining.length > 0) {
        optimisticSendsRef.current[path] = remaining;
        const optWaiting: SessionDetail['waiting_messages'] = remaining.map((opt) => ({
          index: opt.id,
          text: opt.text,
          mode: opt.mode,
          image_count: opt.image_count,
        }));
        waiting_messages = [...waiting_messages, ...optWaiting];
      } else {
        delete optimisticSendsRef.current[path];
      }
    }

    return {
      ...incoming,
      messages,
      artifacts,
      waiting_messages,
    };
  }, []);

  const prefetchTopSessions = useCallback(
    (hostId: string, list: SessionSummary[]) => {
      if (prefetchedHostsRef.current.has(hostId) || list.length === 0) return;
      prefetchedHostsRef.current.add(hostId);
      const entry = liveHostsRef.current.get(hostId);
      if (!entry) return;
      const candidates = list
        .filter((s) => !s.parent_session && !detailsRef.current[s.path])
        .slice(0, 3);
      if (candidates.length === 0) return;
      void (async () => {
        for (const s of candidates) {
          if (detailsRef.current[s.path]) continue;
          const key = `${hostId}::${s.path}`;
          if (inFlightDetailsRef.current.has(key)) continue;
          try {
            const p = entry.transport.getSessionDetail(s.path).then((detail) => {
              const merged = mergeDetail(s.path, detail);
              setDetailsByPath((prev) => ({ ...prev, [s.path]: merged }));
              return merged;
            });
            inFlightDetailsRef.current.set(key, p);
            await p;
          } catch {
            // Best-effort background warm-up; ignore errors.
          } finally {
            inFlightDetailsRef.current.delete(key);
          }
        }
      })();
    },
    [mergeDetail],
  );

  // Hydrate saved hosts and device name on startup.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const storedDevice = await storageGet(DEVICE_NAME_KEY);
        if (!cancelled && storedDevice) setDeviceNameState(storedDevice);
        const rawMeta = await storageGet(HOSTS_KEY);
        const activeId = await storageGet(ACTIVE_HOST_KEY);
        if (rawMeta) {
          const metas = JSON.parse(rawMeta) as StoredHostMeta[];
          const hydrated: SavedHost[] = [];
          for (const m of metas) {
            const token = (await storageGet(tokenKey(m.id))) ?? '';
            hydrated.push({ ...m, token });
          }
          if (!cancelled) {
            hostsRef.current = hydrated;
            setHosts(hydrated);
            setActiveHostId(activeId ?? hydrated[0]?.id ?? null);
          }
        }
      } catch {
        // Fresh install or unavailable storage.
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const activeHost = useMemo(
    () => hosts.find((h) => h.id === activeHostId) ?? hosts[0] ?? null,
    [hosts, activeHostId],
  );

  const clientOrigin = useCallback((): SessionOrigin => {
    const platform =
      Platform.OS === 'ios' ? 'ios' : Platform.OS === 'web' ? 'web' : 'android';
    return {
      platform,
      device: deviceName.trim() || (platform === 'ios' ? 'iPhone' : 'Android'),
      client: 'pui-mobile',
      created_at: Date.now(),
    };
  }, [deviceName]);

  const handleHostWsEvent = useCallback(
    (hostId: string, event: WsServerEvent) => {
      if (event.type === 'sessions_updated') {
        setSessionsByHost((prev) => ({ ...prev, [hostId]: event.sessions }));
        prefetchTopSessions(hostId, event.sessions);
      } else if (event.type === 'session_updated') {
        setDetailsByPath((prev) => ({
          ...prev,
          [event.path]: mergeDetail(event.path, event.detail),
        }));
      } else if (event.type === 'notification') {
      const note = event.notification;
      if (
        shouldPresentNotification(
          note,
          prefsRef.current,
          {
            appState: appStateRef.current,
            activeChatPath: activeChatPathRef.current,
          },
          seenNotificationIdsRef.current,
        )
      ) {
        seenNotificationIdsRef.current.add(note.id);
        if (seenNotificationIdsRef.current.size > 200) {
          const first = seenNotificationIdsRef.current.values().next().value;
          if (first) seenNotificationIdsRef.current.delete(first);
        }
        void presentAgentNotification(note).catch(() => {});
      }
    }
    },
    [mergeDetail, prefetchTopSessions],
  );

  // Keep live transports connected for ALL paired hosts simultaneously (Mac + pui-cloud)
  // so both local and remote cloud sessions stay synced in one unified list.
  useEffect(() => {
    const live = liveHostsRef.current;
    const desiredIds = new Set(hosts.map((h) => h.id));
    let changed = false;

    for (const [id, entry] of live.entries()) {
      if (!desiredIds.has(id)) {
        entry.transport.disconnect();
        live.delete(id);
        changed = true;
      }
    }

    const platform = Platform.OS === 'ios' ? 'ios' : 'android';
    for (const h of hosts) {
      const sig = `${h.host}:${h.port}:${h.tls ? 1 : 0}:${h.token}:${h.transport}:${h.relayUrl ?? ''}:${deviceName}`;
      const existing = live.get(h.id);
      if (existing && existing.sig === sig) continue;
      if (existing) {
        existing.transport.disconnect();
        live.delete(h.id);
      }

      const hostId = h.id;
      const callbacks = {
        onStateChange: (state: ConnectionState, err?: string | null) => {
          setStateByHost((prev) => (prev[hostId] === state ? prev : { ...prev, [hostId]: state }));
          if (err !== undefined) {
            setErrorByHost((prev) => ({ ...prev, [hostId]: err }));
          }
        },
        onEvent: (event: WsServerEvent) => {
          handleHostWsEvent(hostId, event);
        },
      };

      const nextTransport: PuiTransport =
        h.transport === 'cloud_relay'
          ? new CloudRelayTransport(h, callbacks)
          : new DirectTransport(h, callbacks);

      live.set(hostId, { sig, transport: nextTransport });
      changed = true;

      nextTransport.connect(deviceName, platform);
      nextTransport
        .getInfo()
        .then((info) => {
          setHostInfoByHost((prev) => ({ ...prev, [hostId]: info }));
        })
        .catch((err: unknown) => {
          setErrorByHost((prev) => ({
            ...prev,
            [hostId]: err instanceof Error ? err.message : 'Failed to reach Pui host',
          }));
        });
      nextTransport
        .listSessions()
        .then((res) => {
          setSessionsByHost((prev) => ({ ...prev, [hostId]: res.sessions }));
          prefetchTopSessions(hostId, res.sessions);
        })
        .catch(() => {});
    }

    if (changed) {
      setTransportVersion((v) => v + 1);
    }
  }, [hosts, deviceName, handleHostWsEvent, prefetchTopSessions]);

  // Clean up all host transports on unmount.
  useEffect(() => {
    const live = liveHostsRef.current;
    return () => {
      for (const entry of live.values()) {
        entry.transport.disconnect();
      }
      live.clear();
    };
  }, []);

  const persistHosts = useCallback(async (next: SavedHost[], nextActiveId: string | null) => {
    const metas: StoredHostMeta[] = next.map(({ token: _token, ...meta }) => meta);
    await storageSet(HOSTS_KEY, JSON.stringify(metas));
    if (nextActiveId) {
      await storageSet(ACTIVE_HOST_KEY, nextActiveId);
    } else {
      await storageDelete(ACTIVE_HOST_KEY);
    }
  }, []);

  const saveHost = useCallback(
    async (input: Omit<SavedHost, 'id'> & { id?: string }): Promise<SavedHost> => {
      const existingByAddr = hostsRef.current.find(
        (h) => h.host === input.host && h.port === input.port,
      );
      const id =
        input.id ??
        existingByAddr?.id ??
        `host_${Date.now()}_${Math.random().toString(36).slice(2, 7)}`;
      const entry: SavedHost = { ...input, id };
      await storageSet(tokenKey(id), entry.token);
      const updated = [entry, ...hostsRef.current.filter((h) => h.id !== id)];
      hostsRef.current = updated;
      setHosts(updated);
      setActiveHostId(id);
      await persistHosts(updated, id);
      return entry;
    },
    [persistHosts],
  );

  /**
   * Adds (or refreshes the token of) hosts advertised by any connected host — e.g. the Mac's
   * paired cloud host — without switching the active host, so they appear in the unified list.
   */
  const adoptLinkedHosts = useCallback(
    async (linked: LinkedHostInfo[] | undefined) => {
      if (!linked?.length) return;
      let next = hostsRef.current;
      let changed = false;
      for (const l of linked) {
        if (!l?.host || !l.port || !l.token) continue;
        const existing = next.find((h) => h.host === l.host && h.port === l.port);
        if (existing) {
          if (existing.token !== l.token || existing.tls !== Boolean(l.tls)) {
            const updated: SavedHost = { ...existing, token: l.token, tls: Boolean(l.tls) };
            await storageSet(tokenKey(existing.id), updated.token);
            next = next.map((h) => (h.id === existing.id ? updated : h));
            changed = true;
          }
          continue;
        }
        const id = `host_${Date.now()}_${Math.random().toString(36).slice(2, 7)}`;
        const entry: SavedHost = {
          id,
          name: l.name || 'Cloud',
          host: l.host,
          port: l.port,
          tls: Boolean(l.tls),
          token: l.token,
          transport: 'direct',
        };
        await storageSet(tokenKey(id), entry.token);
        next = [...next, entry];
        changed = true;
      }
      if (!changed) return;
      hostsRef.current = next;
      setHosts(next);
      await persistHosts(next, activeHostIdRef.current);
    },
    [persistHosts],
  );

  useEffect(() => {
    for (const info of Object.values(hostInfoByHost)) {
      if (info?.linked_hosts?.length) {
        void adoptLinkedHosts(info.linked_hosts).catch(() => {});
      }
    }
  }, [hostInfoByHost, adoptLinkedHosts]);

  const removeHost = useCallback(
    async (id: string) => {
      await storageDelete(tokenKey(id));
      const remaining = hostsRef.current.filter((h) => h.id !== id);
      hostsRef.current = remaining;
      setHosts(remaining);
      setSessionsByHost((prev) => {
        const next = { ...prev };
        delete next[id];
        return next;
      });
      setHostInfoByHost((prev) => {
        const next = { ...prev };
        delete next[id];
        return next;
      });
      const nextActive = activeHostId === id ? (remaining[0]?.id ?? null) : activeHostId;
      setActiveHostId(nextActive);
      await persistHosts(remaining, nextActive);
    },
    [activeHostId, persistHosts],
  );

  const selectHost = useCallback(async (id: string) => {
    setActiveHostId(id);
    await storageSet(ACTIVE_HOST_KEY, id);
  }, []);

  const setDeviceName = useCallback(async (name: string) => {
    const cleaned = name.trim();
    setDeviceNameState(cleaned);
    await storageSet(DEVICE_NAME_KEY, cleaned);
  }, []);

  const hostInfo = activeHost ? (hostInfoByHost[activeHost.id] ?? null) : null;
  const connectionState: ConnectionState = activeHost
    ? (stateByHost[activeHost.id] ?? 'disconnected')
    : 'disconnected';
  const connectionError: string | null = activeHost
    ? (errorByHost[activeHost.id] ?? null)
    : null;

  const transport = useMemo(() => {
    void transportVersion;
    if (!activeHost) return null;
    return liveHostsRef.current.get(activeHost.id)?.transport ?? null;
  }, [activeHost, transportVersion]);

  const getTransportForHost = useCallback(
    (hostId?: string | null): PuiTransport | null => {
      void transportVersion;
      const targetId = hostId || activeHost?.id;
      if (!targetId) return null;
      return liveHostsRef.current.get(targetId)?.transport ?? null;
    },
    [activeHost?.id, transportVersion],
  );

  const getHostInfoForHost = useCallback(
    (hostId?: string | null): HostInfo | null => {
      const targetId = hostId || activeHost?.id;
      if (!targetId) return null;
      return hostInfoByHost[targetId] ?? null;
    },
    [activeHost?.id, hostInfoByHost],
  );

  // Unified sessions across all paired hosts (Mac + Cloud), deduped by path and sorted newest-first.
  const sessions = useMemo(() => {
    const byPath = new Map<string, SessionSummary>();
    for (const h of hosts) {
      const list = sessionsByHost[h.id] ?? [];
      const info = hostInfoByHost[h.id];
      const hostKind: 'cloud' | 'local' = isCloudHostEntry(h, info) ? 'cloud' : 'local';
      const hostName = info?.host_name || h.name;
      for (const s of list) {
        const tagged: SessionSummary = {
          ...s,
          host_id: h.id,
          host_name: hostName,
          host_kind: hostKind,
        };
        const existing = byPath.get(s.path);
        // Prefer cloud entry if same path somehow appears twice, else newest modified_ms.
        if (!existing || (hostKind === 'cloud' && existing.host_kind !== 'cloud') || tagged.modified_ms > existing.modified_ms) {
          byPath.set(s.path, tagged);
        }
      }
    }
    return Array.from(byPath.values()).sort((a, b) => b.modified_ms - a.modified_ms);
  }, [hosts, sessionsByHost, hostInfoByHost]);

  // Keep Android background foreground-service in sync when any paired host is connected.
  const anyConnected = Object.values(stateByHost).some((s) => s === 'connected');
  useEffect(() => {
    let cancelled = false;
    if (
      activeHost &&
      prefs.notificationsEnabled &&
      prefs.keepAliveInBackground &&
      anyConnected
    ) {
      void getNotificationPermissionStatus().then((status) => {
        if (!cancelled && status === 'granted') {
          startBackgroundKeepAlive(hostInfo?.host_name || activeHost.name);
        }
      });
    } else {
      stopBackgroundKeepAlive();
    }
    return () => {
      cancelled = true;
      if (!activeHost) stopBackgroundKeepAlive();
    };
  }, [
    activeHost,
    hostInfo?.host_name,
    prefs.notificationsEnabled,
    prefs.keepAliveInBackground,
    anyConnected,
  ]);

  const retryConnection = useCallback(() => {
    for (const [hostId, entry] of liveHostsRef.current.entries()) {
      setErrorByHost((prev) => ({ ...prev, [hostId]: null }));
      entry.transport.reconnectNow();
      entry.transport
        .getInfo()
        .then((info) => setHostInfoByHost((prev) => ({ ...prev, [hostId]: info })))
        .catch((err: unknown) => {
          setErrorByHost((prev) => ({
            ...prev,
            [hostId]: err instanceof Error ? err.message : 'Failed to reach Pui host',
          }));
        });
      entry.transport
        .listSessions()
        .then((res) => setSessionsByHost((prev) => ({ ...prev, [hostId]: res.sessions })))
        .catch(() => {});
    }
  }, []);

  const refreshSessions = useCallback(async () => {
    await Promise.all(
      Array.from(liveHostsRef.current.entries()).map(async ([hostId, entry]) => {
        const [info, list] = await Promise.all([
          entry.transport.getInfo().catch(() => null),
          entry.transport.listSessions().catch(() => null),
        ]);
        if (info) {
          setHostInfoByHost((prev) => ({ ...prev, [hostId]: info }));
        }
        if (list) {
          setSessionsByHost((prev) => ({ ...prev, [hostId]: list.sessions }));
        }
      }),
    );
  }, []);

  const loadSessionDetail = useCallback(
    async (path: string, hostId?: string | null): Promise<SessionDetail | null> => {
      const targetId = hostId || activeHost?.id || '';
      const targetTransport = getTransportForHost(hostId);
      if (!targetTransport) return null;
      const key = `${targetId}::${path}`;
      const existing = inFlightDetailsRef.current.get(key);
      if (existing) return existing;
      const promise = targetTransport
        .getSessionDetail(path)
        .then((detail) => {
          const merged = mergeDetail(path, detail);
          setDetailsByPath((prev) => ({ ...prev, [path]: merged }));
          return merged;
        })
        .finally(() => {
          inFlightDetailsRef.current.delete(key);
        });
      inFlightDetailsRef.current.set(key, promise);
      return promise;
    },
    [activeHost?.id, getTransportForHost, mergeDetail],
  );

  /**
   * App-level message delivery that survives navigating away from ChatScreen
   * and immediately places the message into the chat's waiting queue optimistically.
   */
  const sendMessage = useCallback(
    async (
      path: string,
      hostId: string | null | undefined,
      input: {
        text: string;
        images?: { data_base64: string; mime: string; name?: string }[];
        mode?: 'queue' | 'steer';
        origin: SessionOrigin;
      },
    ): Promise<void> => {
      const targetTransport = getTransportForHost(hostId);
      if (!targetTransport) {
        if (input.text.trim()) void saveChatDraft(path, input.text);
        throw new Error('Not connected to Pui host.');
      }

      const optId = nextOptimisticIdRef.current--;
      const mode: 'queue' | 'steer' = input.mode ?? 'queue';
      const curDetail = detailsRef.current[path];
      const optRecord = {
        id: optId,
        text: input.text,
        mode,
        image_count: input.images?.length ?? 0,
        baselineMsgCount: curDetail?.messages.length ?? 0,
      };
      optimisticSendsRef.current[path] = [
        ...(optimisticSendsRef.current[path] ?? []),
        optRecord,
      ];

      // Optimistically show the message in the queue at 0ms.
      setDetailsByPath((prev) => {
        const cur = prev[path];
        if (!cur) return prev;
        return {
          ...prev,
          [path]: {
            ...cur,
            error: null,
            waiting_messages: [
              ...(cur.waiting_messages ?? []),
              {
                index: optId,
                text: input.text,
                mode,
                image_count: input.images?.length ?? 0,
              },
            ],
          },
        };
      });

      const prevRev = curDetail?.revision ?? 0;
      try {
        await targetTransport.sendMessage({
          path,
          text: input.text,
          images: input.images,
          mode,
          origin: input.origin,
        });
        // Fallback sync in case WS session_updated didn't arrive yet.
        setTimeout(() => {
          const latest = detailsRef.current[path];
          if (!latest || latest.revision === prevRev || optimisticSendsRef.current[path]?.length) {
            void loadSessionDetail(path, hostId).catch(() => {});
          }
        }, 250);
      } catch (err: unknown) {
        // Roll back the optimistic entry and preserve the unsent text in draft.
        const remaining = (optimisticSendsRef.current[path] ?? []).filter((o) => o.id !== optId);
        if (remaining.length > 0) {
          optimisticSendsRef.current[path] = remaining;
        } else {
          delete optimisticSendsRef.current[path];
        }
        if (input.text.trim()) {
          void saveChatDraft(path, input.text);
        }
        const message = err instanceof Error ? err.message : 'Failed to send message';
        setDetailsByPath((prev) => {
          const cur = prev[path];
          if (!cur) return prev;
          return {
            ...prev,
            [path]: {
              ...cur,
              error: message,
              waiting_messages: (cur.waiting_messages ?? []).filter((w) => w.index !== optId),
            },
          };
        });
      }
    },
    [getTransportForHost, loadSessionDetail],
  );

  const removeQueuedMessage = useCallback(
    async (path: string, hostId: string | null | undefined, index: number): Promise<void> => {
      const targetTransport = getTransportForHost(hostId);
      if (!targetTransport) return;
      // Optimistically remove from the queue immediately (0ms).
      setDetailsByPath((prev) => {
        const cur = prev[path];
        if (!cur) return prev;
        const next = (cur.waiting_messages ?? [])
          .filter((w) => w.index !== index)
          .map((w, i) => (w.index >= 0 ? { ...w, index: i } : w));
        return { ...prev, [path]: { ...cur, waiting_messages: next } };
      });
      try {
        await targetTransport.removeQueuedMessage(path, index);
        setTimeout(() => void loadSessionDetail(path, hostId).catch(() => {}), 180);
      } catch {
        void loadSessionDetail(path, hostId).catch(() => {});
      }
    },
    [getTransportForHost, loadSessionDetail],
  );

  const value: PuiContextValue = {
    hosts,
    activeHost,
    connectionState,
    connectionError,
    deviceName,
    setDeviceName,
    hostInfo,
    sessions,
    detailsByPath,
    transport,
    getTransportForHost,
    getHostInfoForHost,
    clientOrigin,
    saveHost,
    removeHost,
    selectHost,
    retryConnection,
    refreshSessions,
    loadSessionDetail,
    sendMessage,
    removeQueuedMessage,
    setActiveChatPath,
    pendingShare,
    setPendingShare,
  };

  return <PuiContext.Provider value={value}>{children}</PuiContext.Provider>;
}

export function usePui(): PuiContextValue {
  const ctx = useContext(PuiContext);
  if (!ctx) throw new Error('usePui must be used within PuiProvider');
  return ctx;
}
