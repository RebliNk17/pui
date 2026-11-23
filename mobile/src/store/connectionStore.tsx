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
  refreshSessions: () => Promise<void>;
  loadSessionDetail: (path: string) => Promise<SessionDetail | null>;
}

const PuiContext = createContext<PuiContextValue | null>(null);

export function PuiProvider({ children }: { children: React.ReactNode }) {
  const [hosts, setHosts] = useState<SavedHost[]>([]);
  const [activeHostId, setActiveHostId] = useState<string | null>(null);
  const [deviceName, setDeviceNameState] = useState<string>(
    Device.deviceName ?? Device.modelName ?? (Platform.OS === 'ios' ? 'iPhone' : 'Android Phone'),
  );
  const [connectionState, setConnectionState] = useState<ConnectionState>('disconnected');
  const [connectionError, setConnectionError] = useState<string | null>(null);
  const [hostInfo, setHostInfo] = useState<HostInfo | null>(null);
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [detailsByPath, setDetailsByPath] = useState<Record<string, SessionDetail>>({});
  const transportRef = useRef<PuiTransport | null>(null);

  // Hydrate saved hosts and device name from SecureStore on startup.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const storedDevice = await SecureStore.getItemAsync(DEVICE_NAME_KEY);
        if (!cancelled && storedDevice) setDeviceNameState(storedDevice);
        const rawMeta = await SecureStore.getItemAsync(HOSTS_KEY);
        const activeId = await SecureStore.getItemAsync(ACTIVE_HOST_KEY);
        if (rawMeta) {
          const metas = JSON.parse(rawMeta) as StoredHostMeta[];
          const hydrated: SavedHost[] = [];
          for (const m of metas) {
            const token = (await SecureStore.getItemAsync(tokenKey(m.id))) ?? '';
            hydrated.push({ ...m, token });
          }
          if (!cancelled) {
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
    const platform = Platform.OS === 'ios' ? 'ios' : 'android';
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
    transportRef.current?.disconnect();
    transportRef.current = null;
    setHostInfo(null);
    setSessions([]);
    setDetailsByPath({});
    setConnectionError(null);

    if (!activeHost) {
      setConnectionState('disconnected');
      return;
    }

    const callbacks = {
      onStateChange: (state: ConnectionState, err?: string) => {
        setConnectionState(state);
        setConnectionError(err ?? null);
      },
      onEvent: handleWsEvent,
    };

    const transport: PuiTransport =
      activeHost.transport === 'cloud_relay'
        ? new CloudRelayTransport(activeHost, callbacks)
        : new DirectTransport(activeHost, callbacks);

    transportRef.current = transport;
    const platform = Platform.OS === 'ios' ? 'ios' : 'android';
    transport.connect(deviceName, platform);

    transport
      .getInfo()
      .then((info) => setHostInfo(info))
      .catch((err: unknown) => {
        setConnectionError(err instanceof Error ? err.message : 'Failed to load host info');
      });

    transport
      .listSessions()
      .then((res) => setSessions(res.sessions))
      .catch(() => {});

    return () => {
      transport.disconnect();
    };
  }, [activeHost, deviceName, handleWsEvent]);

  const persistHosts = useCallback(async (next: SavedHost[], nextActiveId: string | null) => {
    const metas: StoredHostMeta[] = next.map(({ token: _token, ...meta }) => meta);
    await SecureStore.setItemAsync(HOSTS_KEY, JSON.stringify(metas));
    if (nextActiveId) {
      await SecureStore.setItemAsync(ACTIVE_HOST_KEY, nextActiveId);
    } else {
      await SecureStore.deleteItemAsync(ACTIVE_HOST_KEY);
    }
  }, []);

  const saveHost = useCallback(
    async (input: Omit<SavedHost, 'id'> & { id?: string }): Promise<SavedHost> => {
      const id = input.id ?? `host_${Date.now()}_${Math.random().toString(36).slice(2, 7)}`;
      const entry: SavedHost = { ...input, id };
      await SecureStore.setItemAsync(tokenKey(id), entry.token);
      let updated: SavedHost[] = [];
      setHosts((prev) => {
        const filtered = prev.filter((h) => h.id !== id);
        updated = [entry, ...filtered];
        return updated;
      });
      setActiveHostId(id);
      await persistHosts(updated, id);
      return entry;
    },
    [persistHosts],
  );

  const removeHost = useCallback(
    async (id: string) => {
      await SecureStore.deleteItemAsync(tokenKey(id));
      let remaining: SavedHost[] = [];
      setHosts((prev) => {
        remaining = prev.filter((h) => h.id !== id);
        return remaining;
      });
      const nextActive = activeHostId === id ? (remaining[0]?.id ?? null) : activeHostId;
      setActiveHostId(nextActive);
      await persistHosts(remaining, nextActive);
    },
    [activeHostId, persistHosts],
  );

  const selectHost = useCallback(async (id: string) => {
    setActiveHostId(id);
    await SecureStore.setItemAsync(ACTIVE_HOST_KEY, id);
  }, []);

  const setDeviceName = useCallback(async (name: string) => {
    const cleaned = name.trim();
    setDeviceNameState(cleaned);
    await SecureStore.setItemAsync(DEVICE_NAME_KEY, cleaned);
  }, []);

  const refreshSessions = useCallback(async () => {
    if (!transportRef.current) return;
    const [info, list] = await Promise.all([
      transportRef.current.getInfo().catch(() => null),
      transportRef.current.listSessions(),
    ]);
    if (info) setHostInfo(info);
    setSessions(list.sessions);
  }, []);

  const loadSessionDetail = useCallback(async (path: string): Promise<SessionDetail | null> => {
    if (!transportRef.current) return null;
    const detail = await transportRef.current.getSessionDetail(path);
    setDetailsByPath((prev) => ({ ...prev, [path]: detail }));
    return detail;
  }, []);

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
    transport: transportRef.current,
    clientOrigin,
    saveHost,
    removeHost,
    selectHost,
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
