import React, { useEffect, useRef, useState } from 'react';
import {
  ActivityIndicator,
  Platform,
  Pressable,
  ScrollView,
  StyleSheet,
  Switch,
  Text,
  TextInput,
  View,
} from 'react-native';
import { router } from 'expo-router';
import { CameraView, useCameraPermissions } from 'expo-camera';
import { Ionicons } from '@expo/vector-icons';
import { CustomScrollbarOverlay, useCustomScrollbar } from '../src/components/CustomScrollbar';
import { usePui } from '../src/store/connectionStore';
import { usePalette } from '../src/theme/colors';
import { testHostConnection } from '../src/transport/transport';
import { normalizeManualHost, parsePairingUri } from '../src/utils/pairing';

type PairTab = 'qr' | 'link' | 'manual';

export default function ConnectScreen() {
  const colors = usePalette();
  const { hosts, activeHost, connectionState, saveHost, selectHost, removeHost } = usePui();
  const { bindings, metrics } = useCustomScrollbar();

  const [tab, setTab] = useState<PairTab>(Platform.OS === 'web' ? 'link' : 'qr');
  const [permission, requestPermission] = useCameraPermissions();
  const [cameraPaused, setCameraPaused] = useState(false);
  const scanLockRef = useRef(false);
  const [connecting, setConnecting] = useState(false);
  const [statusError, setStatusError] = useState<string | null>(null);

  const [pairLinkInput, setPairLinkInput] = useState('');
  const [nameInput, setNameInput] = useState('');
  const [hostInput, setHostInput] = useState('');
  const [portInput, setPortInput] = useState('4817');
  const [tokenInput, setTokenInput] = useState('');
  const [tls, setTls] = useState(false);

  // Reset scan lock whenever returning to the Scan QR tab or resuming camera.
  useEffect(() => {
    if (tab === 'qr' && !cameraPaused && !connecting) {
      scanLockRef.current = false;
    }
  }, [tab, cameraPaused, connecting]);

  const verifyAndConnect = async (candidate: {
    name: string;
    host: string;
    port: number;
    token: string;
    tls: boolean;
  }) => {
    setConnecting(true);
    setStatusError(null);
    try {
      const check = await testHostConnection(candidate);
      if (!check.ok) {
        setStatusError(check.error ?? 'Could not connect to Pui host.');
        setNameInput(candidate.name);
        setHostInput(candidate.host);
        setPortInput(String(candidate.port));
        setTokenInput(candidate.token);
        setTls(candidate.tls);
        setCameraPaused(true);
        return;
      }
      await saveHost({
        name: check.info?.host_name || candidate.name,
        host: candidate.host,
        port: candidate.port,
        token: candidate.token,
        tls: candidate.tls,
        transport: 'direct',
      });
      router.back();
    } catch (err: unknown) {
      setStatusError(err instanceof Error ? err.message : 'Failed to save host.');
      setCameraPaused(true);
    } finally {
      setConnecting(false);
    }
  };

  const handleBarcode = async (data: string) => {
    if (scanLockRef.current || connecting || cameraPaused) return;
    const parsed = parsePairingUri(data);
    if (!parsed) {
      scanLockRef.current = true;
      setCameraPaused(true);
      setStatusError(
        'That QR code is not a Pui pairing code. Scan the QR in Pui Desktop → Settings → Remote & Mobile.',
      );
      return;
    }
    scanLockRef.current = true;
    setCameraPaused(true);
    await verifyAndConnect(parsed);
  };

  const handlePairingLinkConnect = async () => {
    const parsed = parsePairingUri(pairLinkInput);
    if (!parsed) {
      setStatusError(
        'Invalid pairing link. Copy it from Pui Desktop → Settings → Remote & Mobile → Copy pairing link.',
      );
      return;
    }
    await verifyAndConnect(parsed);
  };

  const handleManualSave = async () => {
    const fallbackPort = Number.parseInt(portInput, 10) || 4817;
    const normalized = normalizeManualHost(hostInput, fallbackPort);
    if (!normalized) {
      setStatusError('Enter your Mac Tailscale IP (e.g. 100.108.253.122) or hostname.');
      return;
    }
    if (!tokenInput.trim()) {
      setStatusError('Paste the bearer token from Pui Settings → Remote & Mobile.');
      return;
    }
    await verifyAndConnect({
      name: nameInput.trim() || normalized.host,
      host: normalized.host,
      port: normalized.port,
      tls: tls || normalized.tls,
      token: tokenInput.trim(),
    });
  };

  const showLiveCamera =
    Platform.OS !== 'web' &&
    tab === 'qr' &&
    Boolean(permission?.granted) &&
    !cameraPaused &&
    !connecting;

  return (
    <View style={[styles.root, { backgroundColor: colors.bg }]}>
      <ScrollView {...bindings} contentContainerStyle={styles.container}>
        {/* 3-tab segmented switcher */}
        <View
          style={[
            styles.segment,
            { backgroundColor: colors.surface, borderColor: colors.border },
          ]}
        >
          {(
            [
              ['qr', 'Scan QR'],
              ['link', 'Paste Link'],
              ['manual', 'Manual'],
            ] as [PairTab, string][]
          ).map(([key, label]) => {
            const active = tab === key;
            return (
              <Pressable
                key={key}
                onPress={() => {
                  setStatusError(null);
                  setTab(key);
                }}
                style={[styles.segBtn, active && { backgroundColor: colors.accentBg }]}
              >
                <Text
                  numberOfLines={1}
                  style={{
                    color: active ? colors.accent : colors.muted,
                    fontSize: 12.5,
                    fontWeight: '600',
                  }}
                >
                  {label}
                </Text>
              </Pressable>
            );
          })}
        </View>

        {statusError ? (
          <View
            style={[
              styles.errorBanner,
              { backgroundColor: colors.redBg, borderColor: colors.red },
            ]}
          >
            <Ionicons name="alert-circle-outline" size={18} color={colors.red} />
            <Text style={{ color: colors.red, fontSize: 12.5, lineHeight: 18, flex: 1 }}>
              {statusError}
            </Text>
          </View>
        ) : null}

        {/* TAB 1: Embedded Inline Camera Box */}
        {tab === 'qr' ? (
          <View
            style={[
              styles.cameraFrame,
              { backgroundColor: colors.codeBg, borderColor: colors.border },
            ]}
          >
            {Platform.OS === 'web' ? (
              <View style={styles.cameraPlaceholder}>
                <Ionicons name="qr-code-outline" size={34} color={colors.dim} />
                <Text style={[styles.placeholderTitle, { color: colors.text }]}>
                  Camera unavailable on web
                </Text>
                <Text style={[styles.placeholderSub, { color: colors.muted }]}>
                  Switch to Paste Link or Manual above to pair from a browser.
                </Text>
              </View>
            ) : !permission?.granted ? (
              <View style={styles.cameraPlaceholder}>
                <Ionicons name="camera-outline" size={34} color={colors.accent} />
                <Text style={[styles.placeholderTitle, { color: colors.text }]}>
                  Enable camera to scan QR
                </Text>
                <Text style={[styles.placeholderSub, { color: colors.muted }]}>
                  The viewfinder opens right inside this box to pair with Pui Desktop.
                </Text>
                <Pressable
                  onPress={() => void requestPermission()}
                  style={[styles.inlineCta, { backgroundColor: colors.accent }]}
                >
                  <Text style={styles.inlineCtaText}>Allow camera</Text>
                </Pressable>
              </View>
            ) : (
              <>
                {showLiveCamera ? (
                  <CameraView
                    style={StyleSheet.absoluteFill}
                    facing="back"
                    barcodeScannerSettings={{ barcodeTypes: ['qr'] }}
                    onBarcodeScanned={({ data }) => void handleBarcode(data)}
                  />
                ) : (
                  <View style={styles.cameraPlaceholder}>
                    {connecting ? (
                      <>
                        <ActivityIndicator size="large" color={colors.accent} />
                        <Text style={[styles.placeholderTitle, { color: colors.text }]}>
                          Testing Tailscale connection…
                        </Text>
                      </>
                    ) : (
                      <>
                        <Ionicons name="pause-circle-outline" size={38} color={colors.muted} />
                        <Text style={[styles.placeholderTitle, { color: colors.text }]}>
                          Camera paused
                        </Text>
                        <Pressable
                          onPress={() => {
                            setStatusError(null);
                            scanLockRef.current = false;
                            setCameraPaused(false);
                          }}
                          style={[styles.inlineCta, { backgroundColor: colors.accent }]}
                        >
                          <Text style={styles.inlineCtaText}>Resume scanner</Text>
                        </Pressable>
                      </>
                    )}
                  </View>
                )}

                {/* Viewfinder chrome overlay */}
                {showLiveCamera ? (
                  <View pointerEvents="box-none" style={StyleSheet.absoluteFill}>
                    <View style={styles.camTopRow}>
                      <View style={styles.camBadge}>
                        <View style={[styles.liveDot, { backgroundColor: colors.green }]} />
                        <Text style={styles.camBadgeText}>Camera live</Text>
                      </View>
                      <Pressable
                        onPress={() => setCameraPaused(true)}
                        style={styles.camBadge}
                      >
                        <Text style={styles.camBadgeText}>Pause</Text>
                      </Pressable>
                    </View>

                    <View pointerEvents="none" style={styles.reticleWrap}>
                      <View style={styles.reticle}>
                        <View style={[styles.corner, styles.tl, { borderColor: colors.accent }]} />
                        <View style={[styles.corner, styles.tr, { borderColor: colors.accent }]} />
                        <View style={[styles.corner, styles.bl, { borderColor: colors.accent }]} />
                        <View style={[styles.corner, styles.br, { borderColor: colors.accent }]} />
                      </View>
                    </View>

                    <View pointerEvents="none" style={styles.camBottomHint}>
                      <Text numberOfLines={1} style={styles.camHintText}>
                        Point at QR in Pui → Settings → Remote &amp; Mobile
                      </Text>
                    </View>
                  </View>
                ) : null}
              </>
            )}
          </View>
        ) : null}

        {/* TAB 2: Paste Pairing Link */}
        {tab === 'link' ? (
          <View style={[styles.card, { backgroundColor: colors.surface, borderColor: colors.border }]}>
            <Text style={[styles.cardTitle, { color: colors.text }]}>Paste pairing link</Text>
            <Text style={[styles.cardSub, { color: colors.muted }]}>
              Click &ldquo;Copy pairing link&rdquo; in Pui Desktop → Settings → Remote &amp; Mobile
              and paste it below.
            </Text>
            <TextInput
              value={pairLinkInput}
              onChangeText={setPairLinkInput}
              autoCapitalize="none"
              autoCorrect={false}
              multiline
              placeholder="pui://pair?v=1&host=100.x.y.z&port=4817&token=..."
              placeholderTextColor={colors.dim}
              style={[
                styles.input,
                {
                  minHeight: 74,
                  backgroundColor: colors.bg,
                  color: colors.text,
                  borderColor: colors.border,
                },
              ]}
            />
            <Pressable
              disabled={connecting || !pairLinkInput.trim()}
              onPress={() => void handlePairingLinkConnect()}
              style={[
                styles.primaryBtn,
                {
                  backgroundColor:
                    pairLinkInput.trim() && !connecting ? colors.accent : colors.surfaceElevated,
                },
              ]}
            >
              {connecting ? (
                <ActivityIndicator color="#fff" size="small" />
              ) : (
                <Text
                  style={[
                    styles.primaryBtnText,
                    { color: pairLinkInput.trim() ? '#fff' : colors.dim },
                  ]}
                >
                  Test &amp; Pair
                </Text>
              )}
            </Pressable>
          </View>
        ) : null}

        {/* TAB 3: Manual Host Setup */}
        {tab === 'manual' ? (
          <View style={[styles.card, { backgroundColor: colors.surface, borderColor: colors.border }]}>
            <Text style={[styles.cardTitle, { color: colors.text }]}>Manual Tailscale / LAN host</Text>
            <TextInput
              value={nameInput}
              onChangeText={setNameInput}
              placeholder="Host name (e.g. Shais-MacBook-Pro)"
              placeholderTextColor={colors.dim}
              style={[
                styles.input,
                { backgroundColor: colors.bg, color: colors.text, borderColor: colors.border },
              ]}
            />
            <TextInput
              value={hostInput}
              onChangeText={setHostInput}
              autoCapitalize="none"
              autoCorrect={false}
              placeholder="Tailscale IP or hostname (100.108.253.122)"
              placeholderTextColor={colors.dim}
              style={[
                styles.input,
                { backgroundColor: colors.bg, color: colors.text, borderColor: colors.border },
              ]}
            />
            <View style={styles.row2}>
              <TextInput
                value={portInput}
                onChangeText={setPortInput}
                keyboardType="number-pad"
                placeholder="4817"
                placeholderTextColor={colors.dim}
                style={[
                  styles.input,
                  {
                    width: 96,
                    backgroundColor: colors.bg,
                    color: colors.text,
                    borderColor: colors.border,
                  },
                ]}
              />
              <View style={styles.tlsToggle}>
                <Text numberOfLines={1} style={{ color: colors.muted, fontSize: 12.5 }}>
                  HTTPS / WSS
                </Text>
                <Switch value={tls} onValueChange={setTls} />
              </View>
            </View>
            <TextInput
              value={tokenInput}
              onChangeText={setTokenInput}
              autoCapitalize="none"
              autoCorrect={false}
              secureTextEntry
              placeholder="Bearer token from Pui Settings"
              placeholderTextColor={colors.dim}
              style={[
                styles.input,
                { backgroundColor: colors.bg, color: colors.text, borderColor: colors.border },
              ]}
            />
            <Pressable
              disabled={connecting}
              onPress={() => void handleManualSave()}
              style={[styles.primaryBtn, { backgroundColor: colors.accent }]}
            >
              {connecting ? (
                <ActivityIndicator color="#fff" size="small" />
              ) : (
                <Text style={styles.primaryBtnText}>Test &amp; Connect</Text>
              )}
            </Pressable>
          </View>
        ) : null}

        {/* Saved Hosts list */}
        {hosts.length > 0 ? (
          <>
            <Text style={[styles.sectionLabel, { color: colors.dim }]}>SAVED HOSTS</Text>
            <View
              style={[
                styles.listCard,
                { backgroundColor: colors.surface, borderColor: colors.border },
              ]}
            >
              {hosts.map((h, idx) => {
                const active = activeHost?.id === h.id;
                const live = active && connectionState === 'connected';
                return (
                  <View
                    key={h.id}
                    style={[
                      styles.hostRow,
                      idx > 0 && {
                        borderTopWidth: StyleSheet.hairlineWidth,
                        borderTopColor: colors.hairline,
                      },
                    ]}
                  >
                    <Pressable
                      onPress={async () => {
                        await selectHost(h.id);
                        router.back();
                      }}
                      style={styles.hostLeft}
                    >
                      <View
                        style={[
                          styles.liveDot,
                          { backgroundColor: live ? colors.green : colors.dim },
                        ]}
                      />
                      <View style={{ flex: 1, minWidth: 0 }}>
                        <Text
                          numberOfLines={1}
                          style={{ color: colors.text, fontSize: 14, fontWeight: '600' }}
                        >
                          {h.name}
                        </Text>
                        <Text numberOfLines={1} style={{ color: colors.muted, fontSize: 11.5, marginTop: 2 }}>
                          {h.host}:{h.port}
                          {live ? ' · Connected' : active ? ` · ${connectionState}` : ''}
                        </Text>
                      </View>
                    </Pressable>
                    <View style={styles.hostRight}>
                      {active ? (
                        <Ionicons name="checkmark" size={18} color={colors.accent} />
                      ) : null}
                      <Pressable onPress={() => void removeHost(h.id)} style={{ padding: 6 }}>
                        <Ionicons name="trash-outline" size={17} color={colors.dim} />
                      </Pressable>
                    </View>
                  </View>
                );
              })}
            </View>
          </>
        ) : null}
      </ScrollView>
      <CustomScrollbarOverlay {...metrics} />
    </View>
  );
}

const styles = StyleSheet.create({
  root: {
    flex: 1,
    direction: 'ltr',
  },
  container: {
    padding: 14,
    gap: 12,
    paddingBottom: 32,
  },
  segment: {
    flexDirection: 'row',
    borderWidth: 1,
    borderRadius: 11,
    padding: 3,
    gap: 3,
  },
  segBtn: {
    flex: 1,
    alignItems: 'center',
    paddingVertical: 8,
    borderRadius: 8,
  },
  errorBanner: {
    flexDirection: 'row',
    alignItems: 'flex-start',
    gap: 8,
    padding: 12,
    borderRadius: 11,
    borderWidth: 1,
  },
  cameraFrame: {
    height: 270,
    borderRadius: 18,
    borderWidth: 1,
    overflow: 'hidden',
    position: 'relative',
  },
  cameraPlaceholder: {
    flex: 1,
    alignItems: 'center',
    justifyContent: 'center',
    padding: 24,
    gap: 8,
  },
  placeholderTitle: {
    fontSize: 15,
    fontWeight: '700',
    textAlign: 'center',
  },
  placeholderSub: {
    fontSize: 12.5,
    lineHeight: 18,
    textAlign: 'center',
  },
  inlineCta: {
    marginTop: 6,
    paddingHorizontal: 18,
    paddingVertical: 10,
    borderRadius: 999,
  },
  inlineCtaText: {
    color: '#fff',
    fontSize: 13.5,
    fontWeight: '700',
  },
  camTopRow: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    padding: 12,
  },
  camBadge: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 6,
    backgroundColor: 'rgba(14,14,18,0.72)',
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: 'rgba(255,255,255,0.18)',
    paddingHorizontal: 10,
    paddingVertical: 5,
    borderRadius: 999,
  },
  camBadgeText: {
    color: '#fff',
    fontSize: 11.5,
    fontWeight: '600',
  },
  liveDot: {
    width: 7,
    height: 7,
    borderRadius: 3.5,
    flexShrink: 0,
  },
  reticleWrap: {
    flex: 1,
    alignItems: 'center',
    justifyContent: 'center',
  },
  reticle: {
    width: 150,
    height: 150,
    position: 'relative',
  },
  corner: {
    position: 'absolute',
    width: 24,
    height: 24,
    borderWidth: 0,
  },
  tl: {
    top: 0,
    left: 0,
    borderTopWidth: 3.5,
    borderLeftWidth: 3.5,
    borderTopLeftRadius: 9,
  },
  tr: {
    top: 0,
    right: 0,
    borderTopWidth: 3.5,
    borderRightWidth: 3.5,
    borderTopRightRadius: 9,
  },
  bl: {
    bottom: 0,
    left: 0,
    borderBottomWidth: 3.5,
    borderLeftWidth: 3.5,
    borderBottomLeftRadius: 9,
  },
  br: {
    bottom: 0,
    right: 0,
    borderBottomWidth: 3.5,
    borderRightWidth: 3.5,
    borderBottomRightRadius: 9,
  },
  camBottomHint: {
    alignItems: 'center',
    paddingBottom: 12,
    paddingHorizontal: 12,
  },
  camHintText: {
    color: '#fff',
    fontSize: 11.5,
    fontWeight: '500',
    backgroundColor: 'rgba(14,14,18,0.72)',
    paddingHorizontal: 12,
    paddingVertical: 5,
    borderRadius: 999,
    overflow: 'hidden',
  },
  card: {
    borderWidth: 1,
    borderRadius: 14,
    padding: 14,
    gap: 10,
  },
  cardTitle: {
    fontSize: 15,
    fontWeight: '700',
  },
  cardSub: {
    fontSize: 12.5,
    lineHeight: 18,
  },
  input: {
    borderWidth: 1,
    borderRadius: 10,
    paddingHorizontal: 12,
    paddingVertical: 10,
    fontSize: 13.5,
    textAlign: 'left',
  },
  row2: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 12,
  },
  tlsToggle: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
  },
  primaryBtn: {
    paddingVertical: 12,
    borderRadius: 11,
    alignItems: 'center',
    justifyContent: 'center',
  },
  primaryBtnText: {
    color: '#fff',
    fontSize: 14,
    fontWeight: '700',
  },
  sectionLabel: {
    fontSize: 11,
    fontWeight: '700',
    letterSpacing: 0.7,
    marginTop: 6,
    marginLeft: 4,
  },
  listCard: {
    borderWidth: 1,
    borderRadius: 14,
    overflow: 'hidden',
  },
  hostRow: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    paddingHorizontal: 14,
    paddingVertical: 12,
    gap: 8,
  },
  hostLeft: {
    flex: 1,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 10,
    minWidth: 0,
  },
  hostRight: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 4,
    flexShrink: 0,
  },
});
