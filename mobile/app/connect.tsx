import React, { useRef, useState } from 'react';
import {
  ActivityIndicator,
  Alert,
  Modal,
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
import { usePui } from '../src/store/connectionStore';
import { usePalette } from '../src/theme/colors';
import { testHostConnection } from '../src/transport/transport';
import { normalizeManualHost, parsePairingUri } from '../src/utils/pairing';

export default function ConnectScreen() {
  const colors = usePalette();
  const { hosts, activeHost, deviceName, setDeviceName, saveHost, selectHost, removeHost } =
    usePui();

  const [permission, requestPermission] = useCameraPermissions();
  const [scanning, setScanning] = useState(false);
  const scanLockRef = useRef(false);
  const [connecting, setConnecting] = useState(false);
  const [statusError, setStatusError] = useState<string | null>(null);
  const [pairLinkInput, setPairLinkInput] = useState('');
  const [nameInput, setNameInput] = useState('');
  const [hostInput, setHostInput] = useState('');
  const [portInput, setPortInput] = useState('4817');
  const [tokenInput, setTokenInput] = useState('');
  const [tls, setTls] = useState(false);
  const [deviceInput, setDeviceInput] = useState(deviceName);

  const openScanner = async () => {
    if (Platform.OS === 'web') {
      Alert.alert(
        'Camera scanner unavailable on web',
        'Use "Paste pairing link" or enter your Tailscale IP and token below.',
      );
      return;
    }
    if (!permission?.granted) {
      const res = await requestPermission();
      if (!res.granted) {
        Alert.alert('Camera permission needed', 'Enable camera access to scan the QR code.');
        return;
      }
    }
    scanLockRef.current = false;
    setScanning(true);
  };

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
        // Populate manual fields so the user can inspect/edit immediately.
        setNameInput(candidate.name);
        setHostInput(candidate.host);
        setPortInput(String(candidate.port));
        setTokenInput(candidate.token);
        setTls(candidate.tls);
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
    } finally {
      setConnecting(false);
    }
  };

  const handleBarcode = async (data: string) => {
    if (scanLockRef.current) return;
    const parsed = parsePairingUri(data);
    if (!parsed) {
      scanLockRef.current = true;
      setScanning(false);
      setStatusError('That QR code is not a Pui pairing code (expected pui://pair?...).');
      return;
    }
    scanLockRef.current = true;
    setScanning(false);
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

  return (
    <ScrollView
      style={{ flex: 1, backgroundColor: colors.bg }}
      contentContainerStyle={styles.container}
    >
      <Pressable
        disabled={connecting}
        onPress={() => void openScanner()}
        style={[styles.qrBtn, { backgroundColor: colors.accent }]}
      >
        {connecting ? (
          <ActivityIndicator color="#fff" size="small" />
        ) : (
          <Ionicons name="qr-code-outline" size={20} color="#fff" />
        )}
        <Text style={styles.qrBtnText}>
          {connecting ? 'Testing connection…' : 'Scan QR code from Pui Desktop'}
        </Text>
      </Pressable>

      {statusError ? (
        <View style={[styles.errorBanner, { backgroundColor: colors.redBg, borderColor: colors.red }]}>
          <Ionicons name="alert-circle-outline" size={18} color={colors.red} />
          <Text style={{ color: colors.red, fontSize: 12.5, lineHeight: 18, flex: 1 }}>
            {statusError}
          </Text>
        </View>
      ) : null}

      <View style={[styles.card, { backgroundColor: colors.surface, borderColor: colors.border }]}>
        <Text style={[styles.sectionTitle, { color: colors.text }]}>Paste pairing link</Text>
        <Text style={[styles.hint, { color: colors.muted }]}>
          Click &ldquo;Copy pairing link&rdquo; in Pui Desktop and paste it here to connect in one tap.
        </Text>
        <View style={styles.deviceRow}>
          <TextInput
            value={pairLinkInput}
            onChangeText={setPairLinkInput}
            autoCapitalize="none"
            autoCorrect={false}
            placeholder="pui://pair?v=1&host=100.x.y.z&..."
            placeholderTextColor={colors.dim}
            style={[
              styles.input,
              { flex: 1, backgroundColor: colors.codeBg, color: colors.text, borderColor: colors.border },
            ]}
          />
          <Pressable
            disabled={connecting || !pairLinkInput.trim()}
            onPress={() => void handlePairingLinkConnect()}
            style={[styles.smallBtn, { backgroundColor: colors.accent, borderColor: colors.accent }]}
          >
            <Text style={{ color: '#fff', fontWeight: '700', fontSize: 12.5 }}>Pair</Text>
          </Pressable>
        </View>
      </View>

      <View style={[styles.card, { backgroundColor: colors.surface, borderColor: colors.border }]}>
        <Text style={[styles.sectionTitle, { color: colors.text }]}>This device</Text>
        <Text style={[styles.hint, { color: colors.muted }]}>
          Recorded on every session you start from this phone so Desktop and Mobile know where it
          originated.
        </Text>
        <View style={styles.deviceRow}>
          <TextInput
            value={deviceInput}
            onChangeText={setDeviceInput}
            placeholder="Pixel 9"
            placeholderTextColor={colors.dim}
            style={[
              styles.input,
              { flex: 1, backgroundColor: colors.codeBg, color: colors.text, borderColor: colors.border },
            ]}
          />
          <Pressable
            onPress={() => void setDeviceName(deviceInput)}
            style={[styles.smallBtn, { backgroundColor: colors.surfaceElevated, borderColor: colors.border }]}
          >
            <Text style={{ color: colors.text, fontWeight: '600', fontSize: 12.5 }}>Save</Text>
          </Pressable>
        </View>
      </View>

      <View style={[styles.card, { backgroundColor: colors.surface, borderColor: colors.border }]}>
        <Text style={[styles.sectionTitle, { color: colors.text }]}>
          Manual Tailscale / LAN host
        </Text>
        <TextInput
          value={nameInput}
          onChangeText={setNameInput}
          placeholder="Host label (e.g. Shai's MacBook)"
          placeholderTextColor={colors.dim}
          style={[
            styles.input,
            { backgroundColor: colors.codeBg, color: colors.text, borderColor: colors.border },
          ]}
        />
        <TextInput
          value={hostInput}
          onChangeText={setHostInput}
          autoCapitalize="none"
          autoCorrect={false}
          placeholder="Tailscale IP or hostname (e.g. 100.108.253.122)"
          placeholderTextColor={colors.dim}
          style={[
            styles.input,
            { backgroundColor: colors.codeBg, color: colors.text, borderColor: colors.border },
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
              { flex: 1, backgroundColor: colors.codeBg, color: colors.text, borderColor: colors.border },
            ]}
          />
          <View style={styles.tlsToggle}>
            <Text style={{ color: colors.muted, fontSize: 12.5 }}>HTTPS/WSS</Text>
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
            { backgroundColor: colors.codeBg, color: colors.text, borderColor: colors.border },
          ]}
        />
        <Pressable
          disabled={connecting}
          onPress={() => void handleManualSave()}
          style={[styles.saveBtn, { backgroundColor: colors.accent }]}
        >
          <Text style={styles.qrBtnText}>
            {connecting ? 'Testing connection…' : 'Test & connect host'}
          </Text>
        </Pressable>
      </View>

      {hosts.length > 0 ? (
        <View style={[styles.card, { backgroundColor: colors.surface, borderColor: colors.border }]}>
          <Text style={[styles.sectionTitle, { color: colors.text }]}>Saved hosts</Text>
          {hosts.map((h) => {
            const active = activeHost?.id === h.id;
            return (
              <View key={h.id} style={[styles.hostRow, { borderTopColor: colors.border }]}>
                <Pressable
                  onPress={async () => {
                    await selectHost(h.id);
                    router.back();
                  }}
                  style={{ flex: 1 }}
                >
                  <Text style={{ color: colors.text, fontSize: 14, fontWeight: '600' }}>
                    {h.name} {active ? '· Active' : ''}
                  </Text>
                  <Text style={{ color: colors.dim, fontSize: 11.5 }}>
                    {h.tls ? 'wss' : 'ws'}://{h.host}:{h.port} ({h.transport})
                  </Text>
                </Pressable>
                <Pressable onPress={() => void removeHost(h.id)} style={{ padding: 6 }}>
                  <Ionicons name="trash-outline" size={18} color={colors.red} />
                </Pressable>
              </View>
            );
          })}
        </View>
      ) : null}

      <Modal visible={scanning} animationType="slide" onRequestClose={() => setScanning(false)}>
        <View style={{ flex: 1, backgroundColor: '#000' }}>
          <CameraView
            style={{ flex: 1 }}
            barcodeScannerSettings={{ barcodeTypes: ['qr'] }}
            onBarcodeScanned={({ data }) => void handleBarcode(data)}
          />
          <Pressable onPress={() => setScanning(false)} style={styles.cancelScan}>
            <Text style={{ color: '#fff', fontSize: 15, fontWeight: '700' }}>Cancel</Text>
          </Pressable>
        </View>
      </Modal>
    </ScrollView>
  );
}

const styles = StyleSheet.create({
  container: {
    padding: 14,
    gap: 14,
  },
  qrBtn: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'center',
    gap: 8,
    paddingVertical: 14,
    borderRadius: 12,
  },
  qrBtnText: {
    color: '#fff',
    fontSize: 14.5,
    fontWeight: '700',
  },
  errorBanner: {
    flexDirection: 'row',
    alignItems: 'flex-start',
    gap: 8,
    padding: 12,
    borderRadius: 10,
    borderWidth: 1,
  },
  card: {
    borderWidth: 1,
    borderRadius: 12,
    padding: 14,
    gap: 10,
  },
  sectionTitle: {
    fontSize: 15,
    fontWeight: '700',
  },
  hint: {
    fontSize: 12,
    lineHeight: 17,
  },
  deviceRow: {
    flexDirection: 'row',
    gap: 8,
    alignItems: 'center',
  },
  smallBtn: {
    paddingHorizontal: 14,
    paddingVertical: 10,
    borderRadius: 9,
    borderWidth: 1,
  },
  input: {
    borderWidth: 1,
    borderRadius: 9,
    paddingHorizontal: 11,
    paddingVertical: 10,
    fontSize: 13.5,
  },
  row2: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 12,
  },
  tlsToggle: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 6,
  },
  saveBtn: {
    paddingVertical: 12,
    borderRadius: 10,
    alignItems: 'center',
  },
  hostRow: {
    flexDirection: 'row',
    alignItems: 'center',
    paddingTop: 10,
    borderTopWidth: StyleSheet.hairlineWidth,
  },
  cancelScan: {
    position: 'absolute',
    bottom: 42,
    alignSelf: 'center',
    backgroundColor: 'rgba(0,0,0,0.75)',
    paddingHorizontal: 24,
    paddingVertical: 12,
    borderRadius: 999,
  },
});
