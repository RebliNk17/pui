import React, { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react';
import { Platform } from 'react-native';
import * as Device from 'expo-device';
import * as SecureStore from 'expo-secure-store';
import type {
  HostInfo,
  SavedHost,
  SessionDetail,
  SessionOrigin,
  SessionSummary,
  WsServerEvent,
} from '../protocol/types';
import {
  CloudRelayTransport,
  DirectTransport,
  type ConnectionState,
  type PuiTransport,
} from '../transport/transport';

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
  sessions: SessionSummary[];
  detailsByPath: Record<string, SessionDetail>;
  transport: PuiTransport | null;
  clientOrigin: () => SessionOrigin;
  saveHost: (host: Omit<SavedHost, 'id'> & { id?: string }) => Promise<SavedHost>;
  removeHost: (id: string) => Promise<void>;
  selectHost: (id: string) => Promise<void>;
  retryConnection: () => void;
  refreshSessions: () => Promise<void>;
  loadSessionDetail: (path: string) => Promise<SessionDetail | null>;
}

const PuiContext = createContext<PuiContextValue | null>(null);

export function PuiProvider({ children }: { children: React.ReactNode }) {
  const [hosts, setHosts] = useState<SavedHost[]>([]);
  const hostsRef = useRef<SavedHost[]>([]);
  const [activeHostId, setActiveHostId] = useState<string | null>(null);
  const [deviceName, setDeviceNameState] = useState<string>(
    Device.deviceName ?? Device.modelName ?? (Platform.OS === 'ios' ? 'iPhone' : 'Android Phone'),
  );
  const [connectionState, setConnectionState] = useState<ConnectionState>('disconnected');
  const [connectionError, setConnectionError] = useState<string | null>(null);
  const [hostInfo, setHostInfo] = useState<HostInfo | null>(null);
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [detailsByPath, setDetailsByPath] = useState<Record<string, SessionDetail>>({});
  const [transport, setTransport] = useState<PuiTransport | null>(null);

  useEffect(() => {
    hostsRef.current = hosts;
  }, [hosts]);

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
    () => hosts.find((h) => h.id === activeHostId) ?? null,
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

  const handleWsEvent = useCallback((event: WsServerEvent) => {
    if (event.type === 'sessions_updated') {
      setSessions(event.sessions);
    } else if (event.type === 'session_updated') {
      setDetailsByPath((prev) => ({ ...prev, [event.path]: event.detail }));
    }
  }, []);

  // Connect transport whenever the active host changes.
  useEffect(() => {
    setHostInfo(null);
    setSessions([]);
    setDetailsByPath({});
    setConnectionError(null);

    if (!activeHost) {
      setTransport(null);
      setConnectionState('disconnected');
      return;
    }

    let disposed = false;
    const callbacks = {
      onStateChange: (state: ConnectionState, err?: string | null) => {
        if (disposed) return;
        setConnectionState(state);
        if (err !== undefined) {
          setConnectionError(err);
        }
      },
      onEvent: (event: WsServerEvent) => {
        if (!disposed) handleWsEvent(event);
      },
    };

    const nextTransport: PuiTransport =
      activeHost.transport === 'cloud_relay'
        ? new CloudRelayTransport(activeHost, callbacks)
        : new DirectTransport(activeHost, callbacks);

    setTransport(nextTransport);
    const platform = Platform.OS === 'ios' ? 'ios' : 'android';
    nextTransport.connect(deviceName, platform);

    nextTransport
      .getInfo()
      .then((info) => {
        if (!disposed) setHostInfo(info);
      })
      .catch((err: unknown) => {
        if (!disposed) {
          setConnectionError(err instanceof Error ? err.message : 'Failed to reach Pui host');
        }
      });

    nextTransport
      .listSessions()
      .then((res) => {
        if (!disposed) setSessions(res.sessions);
      })
      .catch(() => {});

    return () => {
      disposed = true;
      nextTransport.disconnect();
    };
  }, [activeHost, deviceName, handleWsEvent]);

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

  const removeHost = useCallback(
    async (id: string) => {
      await storageDelete(tokenKey(id));
      const remaining = hostsRef.current.filter((h) => h.id !== id);
      hostsRef.current = remaining;
      setHosts(remaining);
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

  const retryConnection = useCallback(() => {
    if (!transport) return;
    setConnectionError(null);
    transport.reconnectNow();
    transport
      .getInfo()
      .then((info) => setHostInfo(info))
      .catch((err: unknown) => {
        setConnectionError(err instanceof Error ? err.message : 'Failed to reach Pui host');
      });
    transport
      .listSessions()
      .then((res) => setSessions(res.sessions))
      .catch(() => {});
  }, [transport]);

  const refreshSessions = useCallback(async () => {
    if (!transport) return;
    const [info, list] = await Promise.all([
      transport.getInfo().catch(() => null),
      transport.listSessions(),
    ]);
    if (info) setHostInfo(info);
    setSessions(list.sessions);
  }, [transport]);

  const loadSessionDetail = useCallback(
    async (path: string): Promise<SessionDetail | null> => {
      if (!transport) return null;
      const detail = await transport.getSessionDetail(path);
      setDetailsByPath((prev) => ({ ...prev, [path]: detail }));
      return detail;
    },
    [transport],
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
    clientOrigin,
    saveHost,
    removeHost,
    selectHost,
    retryConnection,
    refreshSessions,
    loadSessionDetail,
  };

  return <PuiContext.Provider value={value}>{children}</PuiContext.Provider>;
}

export function usePui(): PuiContextValue {
  const ctx = useContext(PuiContext);
  if (!ctx) throw new Error('usePui must be used within PuiProvider');
  return ctx;
}
