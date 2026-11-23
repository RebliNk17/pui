import React, { useMemo, useState } from 'react';
import { Modal, Pressable, ScrollView, StyleSheet, Text, View } from 'react-native';
import { Ionicons } from '@expo/vector-icons';
import { WebView } from 'react-native-webview';
import { usePalette } from '../theme/colors';
import { wrapSandboxedHtml } from '../utils/htmlSandbox';

interface Props {
  title: string;
  html: string;
  initialHeight?: number;
}

/**
 * Sandboxed inline HTML/CSS/SVG card for `pui_visual` and `.html` artifacts.
 * Runs in an isolated opaque origin with strict CSP — no external network or full browser UI.
 */
export function HtmlArtifactCard({ title, html, initialHeight }: Props) {
  const colors = usePalette();
  const [height, setHeight] = useState<number>(Math.min(Math.max(initialHeight ?? 240, 140), 420));
  const [showSource, setShowSource] = useState(false);
  const [expanded, setExpanded] = useState(false);

  const wrappedHtml = useMemo(() => wrapSandboxedHtml(html, colors.dark), [html, colors.dark]);

  const handleMessage = (event: { nativeEvent: { data: string } }) => {
    try {
      const msg = JSON.parse(event.nativeEvent.data) as { type?: string; height?: number };
      if (msg.type === 'height' && typeof msg.height === 'number' && msg.height > 0) {
        setHeight(Math.min(Math.max(msg.height, 140), 520));
      }
    } catch {
      // ignore
    }
  };

  return (
    <View style={[styles.card, { backgroundColor: colors.surface, borderColor: colors.border }]}>
      <View style={[styles.header, { borderBottomColor: colors.border }]}>
        <View style={styles.titleRow}>
          <Ionicons name="sparkles-outline" size={14} color={colors.accent} />
          <Text numberOfLines={1} style={[styles.title, { color: colors.text }]}>
            {title || 'Interactive visual'}
          </Text>
        </View>
        <View style={styles.actions}>
          <Pressable
            onPress={() => setShowSource((v) => !v)}
            style={[styles.btn, { backgroundColor: colors.surfaceElevated }]}
          >
            <Text style={[styles.btnText, { color: colors.muted }]}>
              {showSource ? 'Preview' : 'Source'}
            </Text>
          </Pressable>
          <Pressable
            onPress={() => setExpanded(true)}
            style={[styles.btn, { backgroundColor: colors.surfaceElevated }]}
          >
            <Ionicons name="expand-outline" size={14} color={colors.muted} />
          </Pressable>
        </View>
      </View>

      {showSource ? (
        <ScrollView style={[styles.sourceBox, { backgroundColor: colors.codeBg }]}>
          <Text selectable style={[styles.sourceText, { color: colors.text }]}>
            {html}
          </Text>
        </ScrollView>
      ) : (
        <View style={{ height, overflow: 'hidden' }}>
          <WebView
            originWhitelist={['about:blank', 'data:*']}
            source={{ html: wrappedHtml, baseUrl: 'about:blank' }}
            javaScriptEnabled
            domStorageEnabled={false}
            allowFileAccess={false}
            mixedContentMode="never"
            onShouldStartLoadWithRequest={(req) => req.url === 'about:blank'}
            onMessage={handleMessage}
            scrollEnabled={false}
            style={{ flex: 1, backgroundColor: 'transparent' }}
          />
        </View>
      )}

      <Modal visible={expanded} animationType="slide" onRequestClose={() => setExpanded(false)}>
        <View style={[styles.modalContainer, { backgroundColor: colors.bg }]}>
          <View style={[styles.modalHeader, { borderBottomColor: colors.border }]}>
            <Text numberOfLines={1} style={[styles.modalTitle, { color: colors.text }]}>
              {title || 'Artifact preview'}
            </Text>
            <Pressable
              onPress={() => setExpanded(false)}
              style={[styles.btn, { backgroundColor: colors.surfaceElevated }]}
            >
              <Ionicons name="close" size={18} color={colors.text} />
            </Pressable>
          </View>
          <WebView
            originWhitelist={['about:blank', 'data:*']}
            source={{ html: wrappedHtml, baseUrl: 'about:blank' }}
            javaScriptEnabled
            domStorageEnabled={false}
            allowFileAccess={false}
            onShouldStartLoadWithRequest={(req) => req.url === 'about:blank'}
            style={{ flex: 1, backgroundColor: colors.bg }}
          />
        </View>
      </Modal>
    </View>
  );
}

const styles = StyleSheet.create({
  card: {
    borderWidth: 1,
    borderRadius: 10,
    overflow: 'hidden',
    marginTop: 8,
  },
  header: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    paddingHorizontal: 10,
    paddingVertical: 8,
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  titleRow: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 6,
    flex: 1,
    marginRight: 8,
  },
  title: {
    fontSize: 12.5,
    fontWeight: '600',
    flex: 1,
  },
  actions: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 6,
  },
  btn: {
    paddingHorizontal: 8,
    paddingVertical: 4,
    borderRadius: 6,
  },
  btnText: {
    fontSize: 11,
    fontWeight: '600',
  },
  sourceBox: {
    maxHeight: 260,
    padding: 10,
  },
  sourceText: {
    fontFamily: 'monospace',
    fontSize: 11,
    lineHeight: 16,
  },
  modalContainer: {
    flex: 1,
    paddingTop: 46,
  },
  modalHeader: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    paddingHorizontal: 16,
    paddingVertical: 12,
    borderBottomWidth: 1,
  },
  modalTitle: {
    fontSize: 15,
    fontWeight: '600',
    flex: 1,
    marginRight: 12,
  },
});
