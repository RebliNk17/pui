import React, { useState } from 'react';
import {
  Alert,
  Modal,
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
import { normalizeManualHost, parsePairingUri } from '../src/utils/pairing';

export default function ConnectScreen() {
  const colors = usePalette();
  const { hosts, activeHost, deviceName, setDeviceName, saveHost, selectHost, removeHost } =
    usePui();

  const [permission, requestPermission] = useCameraPermissions();
  const [scanning, setScanning] = useState(false);
  const [nameInput, setNameInput] = useState('');
  const [hostInput, setHostInput] = useState('');
  const [portInput, setPortInput] = useState('4817');
  const [tokenInput, setTokenInput] = useState('');
  const [tls, setTls] = useState(false);
  const [deviceInput, setDeviceInput] = useState(deviceName);

  const openScanner = async () => {
    if (!permission?.granted) {
      const res = await requestPermission();
      if (!res.granted) {
        Alert.alert('Camera permission needed', 'Enable camera access to scan the QR code.');
        return;
      }
    }
    setScanning(true);
  };

  const handleBarcode = async (data: string) => {
    const parsed = parsePairingUri(data);
    if (!parsed) return;
    setScanning(false);
    await saveHost({
      name: parsed.name,
      host: parsed.host,
      port: parsed.port,
      token: parsed.token,
      tls: parsed.tls,
      transport: 'direct',
    });
    router.back();
  };

  const handleManualSave = async () => {
    const fallbackPort = Number.parseInt(portInput, 10) || 4817;
    const normalized = normalizeManualHost(hostInput, fallbackPort);
    if (!normalized) {
      Alert.alert('Invalid host', 'Enter your Mac Tailscale IP (e.g. 100.x.y.z) or hostname.');
      return;
    }
    if (!tokenInput.trim()) {
      Alert.alert('Token required', 'Paste the bearer token from Pui Settings → Remote & Mobile.');
      return;
    }
    await saveHost({
      name: nameInput.trim() || normalized.host,
      host: normalized.host,
      port: normalized.port,
      tls: tls || normalized.tls,
      token: tokenInput.trim(),
      transport: 'direct',
    });
    router.back();
  };

  return (
    <ScrollView
      style={{ flex: 1, backgroundColor: colors.bg }}
      contentContainerStyle={styles.container}
    >
      <Pressable
        onPress={() => void openScanner()}
        style={[styles.qrBtn, { backgroundColor: colors.accent }]}
      >
        <Ionicons name="qr-code-outline" size={20} color="#fff" />
        <Text style={styles.qrBtnText}>Scan QR code from Pui Desktop</Text>
      </Pressable>

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
          placeholder="Tailscale IP or hostname (e.g. 100.88.12.3)"
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
          onPress={() => void handleManualSave()}
          style={[styles.saveBtn, { backgroundColor: colors.accent }]}
        >
          <Text style={styles.qrBtnText}>Connect host</Text>
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
